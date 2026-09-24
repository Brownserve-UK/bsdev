#!/usr/bin/env node
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs, stripVTControlCharacters } from 'node:util';

const SELF = fileURLToPath(import.meta.url);
const RUNTIME = path.dirname(path.dirname(SELF));
const NODE_MODULES = path.join(RUNTIME, 'node_modules');
const DEFAULT_PORT = 5199;
const PROTOTYPES = path.join('.agents', 'prototypes');
const STATE_DIR = path.join(process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache'), 'bsdev-proto');
const START_TIMEOUT_MS = 30000;

const USAGE = `Usage: bsdev-proto <command> [options]

Serves <repo>/${PROTOTYPES} with live reload on 127.0.0.1.

Commands:
  start [repo]         Start (or reuse) the server for a repo, print its URL last
  stop [repo]          Stop the server for a repo
  status               List running servers
  url [repo]           Print the URL of a running server
  logs [repo]          Print the server log (Vite compile errors land here)
  shot <page> [repo]   Screenshot a prototype with Playwright, print the PNG path

Options:
  --no-open            start: don't open the host browser from a VSCode terminal
  --out <file>         shot: output PNG path
  --viewport <WxH>     shot: viewport size (default 412x915)
  --full-page          shot: capture the full scrollable page
  -h, --help           Show this help

[repo] defaults to the git root of the current directory (or the directory itself).`;

function fail(message) {
    console.error(`bsdev-proto: ${message}`);
    process.exit(1);
}

function repoRoot(dir) {
    const from = path.resolve(dir || process.cwd());
    if (!fs.existsSync(from)) fail(`no such directory: ${from}`);
    const base = fs.realpathSync(from);
    for (let d = base; ; d = path.dirname(d)) {
        if (fs.existsSync(path.join(d, '.git'))) return d;
        if (path.dirname(d) === d) return base;
    }
}

function key(repo) {
    return createHash('sha1').update(repo).digest('hex').slice(0, 16);
}

function paths(repo) {
    const k = key(repo);
    return {
        state: path.join(STATE_DIR, `${k}.json`),
        log: path.join(STATE_DIR, `${k}.log`),
        cache: path.join(STATE_DIR, k),
    };
}

function alive(pid) {
    try {
        const cmd = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0');
        return cmd.includes(SELF) && cmd.includes('serve');
    } catch {
        return false;
    }
}

function readState(file) {
    let state;
    try {
        state = JSON.parse(fs.readFileSync(file, 'utf8'));
    } catch {
        return null;
    }
    if (!state.pid || !alive(state.pid)) {
        fs.rmSync(file, { force: true });
        return null;
    }
    return state;
}

function url(port) {
    return `http://localhost:${port}/`;
}

function inVSCode() {
    return process.env.TERM_PROGRAM === 'vscode' || Boolean(process.env.VSCODE_IPC_HOOK_CLI);
}

function openBrowser(target) {
    const browser = process.env.BROWSER;
    if (!browser) return false;
    try {
        const child = spawn(browser, [target], { detached: true, stdio: 'ignore' });
        child.on('error', () => {});
        child.unref();
        return true;
    } catch {
        return false;
    }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function tail(file, lines = 30) {
    try {
        return stripVTControlCharacters(fs.readFileSync(file, 'utf8')).trimEnd().split('\n').slice(-lines).join('\n');
    } catch {
        return '';
    }
}

async function ensureStarted(repo) {
    const p = paths(repo);
    const existing = readState(p.state);
    if (existing) return { state: existing, fresh: false };

    const root = path.join(repo, PROTOTYPES);
    fs.mkdirSync(root, { recursive: true });
    fs.mkdirSync(STATE_DIR, { recursive: true });

    const log = fs.openSync(p.log, 'w');
    const child = spawn(process.execPath, [SELF, 'serve', repo], {
        cwd: root,
        detached: true,
        stdio: ['ignore', log, log],
    });
    fs.closeSync(log);

    let exited = null;
    child.on('exit', (code, signal) => {
        exited = signal || code;
    });
    child.unref();

    const deadline = Date.now() + START_TIMEOUT_MS;
    while (Date.now() < deadline) {
        const state = readState(p.state);
        if (state && state.pid === child.pid) return { state, fresh: true };
        if (exited !== null) break;
        await sleep(100);
    }
    if (exited === null) {
        try {
            process.kill(child.pid, 'SIGTERM');
        } catch {}
    }
    const logTail = tail(p.log);
    fail(`server failed to start${exited === null ? ' (timed out)' : ''}, see ${p.log}${logTail ? `\n${logTail}` : ''}`);
}

async function start(repo, opts) {
    const { state, fresh } = await ensureStarted(repo);
    const target = url(state.port);
    console.log(`${fresh ? 'serving' : 'already serving'} ${state.root}`);
    if (inVSCode()) {
        if (!opts['no-open'] && fresh && !openBrowser(target)) {
            console.log('open it in your host browser (VSCode forwards the port automatically)');
        }
    } else {
        console.log(`run on host: bsdev forward ${state.port}`);
    }
    console.log(target);
}

async function stop(repo) {
    const p = paths(repo);
    const state = readState(p.state);
    if (!state) {
        console.log(`not running for ${repo}`);
        return;
    }
    process.kill(state.pid, 'SIGTERM');
    for (let i = 0; i < 50 && alive(state.pid); i++) await sleep(100);
    if (alive(state.pid)) process.kill(state.pid, 'SIGKILL');
    fs.rmSync(p.state, { force: true });
    console.log(`stopped ${state.root} (port ${state.port})`);
}

function status() {
    let files = [];
    try {
        files = fs.readdirSync(STATE_DIR).filter((f) => f.endsWith('.json'));
    } catch {}
    const running = files.map((f) => readState(path.join(STATE_DIR, f))).filter(Boolean);
    if (running.length === 0) {
        console.log('no servers running');
        return;
    }
    for (const s of running) console.log(`${url(s.port)}  pid ${s.pid}  ${s.root}`);
}

function printUrl(repo) {
    const state = readState(paths(repo).state);
    if (!state) fail(`not running for ${repo}, use: bsdev-proto start`);
    console.log(url(state.port));
}

function logs(repo) {
    const file = paths(repo).log;
    if (!fs.existsSync(file)) fail(`no log for ${repo}`);
    process.stdout.write(stripVTControlCharacters(fs.readFileSync(file, 'utf8')));
}

function pagePath(page) {
    const clean = (page || '').replace(/^\/+/, '');
    if (!clean) return '';
    return path.posix.extname(clean) ? clean : `${clean.replace(/\/+$/, '')}/`;
}

async function shot(page, repo, opts) {
    if (!page) fail('shot needs a page, e.g. bsdev-proto shot my-redesign');
    const viewport = (opts.viewport || '412x915').match(/^(\d+)x(\d+)$/);
    if (!viewport) fail(`bad --viewport "${opts.viewport}", expected WxH e.g. 412x915`);

    const { state } = await ensureStarted(repo);
    const slug = page.replace(/[^\w.-]+/g, '_').replace(/^_+|_+$/g, '') || 'index';
    const out = path.resolve(opts.out || path.join(paths(repo).cache, 'shots', `${slug}.png`));
    fs.mkdirSync(path.dirname(out), { recursive: true });

    const args = [
        'screenshot',
        `--viewport-size=${viewport[1]}, ${viewport[2]}`,
        '--wait-for-timeout=1000',
        ...(opts['full-page'] ? ['--full-page'] : []),
        `http://127.0.0.1:${state.port}/${pagePath(page)}`,
        out,
    ];
    const env = { ...process.env, PLAYWRIGHT_BROWSERS_PATH: process.env.PLAYWRIGHT_BROWSERS_PATH || '/opt/playwright' };
    const result = spawnSync('playwright', args, { stdio: ['ignore', 'inherit', 'inherit'], env });
    if (result.error) fail(`couldn't run playwright: ${result.error.message}`);
    if (result.status !== 0) fail(`playwright screenshot exited with ${result.status}`);
    console.log(out);
}

function runtimeResolution() {
    const pkg = JSON.parse(fs.readFileSync(path.join(RUNTIME, 'package.json'), 'utf8'));
    const deps = Object.keys(pkg.dependencies || {});
    const escape = (s) => s.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');
    const cssPackages = deps.filter((d) => fs.existsSync(path.join(NODE_MODULES, d, 'index.css')));
    const alias = cssPackages.flatMap((d) => [
        { find: new RegExp(`^${escape(d)}$`), replacement: path.join(NODE_MODULES, d, 'index.css') },
        { find: new RegExp(`^${escape(d)}/(.*)$`), replacement: `${path.join(NODE_MODULES, d)}/$1` },
    ]);
    const jsPackages = deps.filter((d) => !cssPackages.includes(d));
    const bare = new RegExp(`^(?:${jsPackages.map(escape).join('|')})(?:/.*)?$`);
    const anchor = path.join(RUNTIME, 'package.json');
    const plugin = {
        name: 'bsdev-proto:runtime',
        enforce: 'pre',
        async resolveId(id, importer, options) {
            if (!bare.test(id) || importer === anchor) return null;
            return this.resolve(id, anchor, { ...options, skipSelf: true });
        },
    };
    const optimizeDeps = {
        name: 'bsdev-proto:optimize-deps',
        enforce: 'post',
        config(config) {
            if (config.optimizeDeps?.include) {
                config.optimizeDeps.include = config.optimizeDeps.include.filter((id) => !bare.test(id));
            }
        },
    };
    return { alias, plugins: [plugin, optimizeDeps] };
}

function indexPlugin(root) {
    const page = (items) => `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Prototypes</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:40rem;margin:3rem auto;padding:0 1rem;color:#1f2937}
a{display:block;padding:.75rem 1rem;margin:.5rem 0;border:1px solid #e5e7eb;border-radius:.5rem;color:inherit;text-decoration:none}
a:hover{background:#f3f4f6}</style></head>
<body><h1>Prototypes</h1>${items.length ? items.map((n) => `<a href="./${encodeURI(n)}/">${n}</a>`).join('') : `<p>Nothing here yet. Add <code>&lt;name&gt;/index.html</code> to <code>${root}</code>.</p>`}</body></html>`;
    return {
        name: 'bsdev-proto:index',
        configureServer(server) {
            server.middlewares.use((req, res, next) => {
                const pathname = (req.url || '/').split('?')[0];
                if (pathname !== '/' || fs.existsSync(path.join(root, 'index.html'))) return next();
                const items = fs
                    .readdirSync(root, { withFileTypes: true })
                    .filter((e) => e.isDirectory() && fs.existsSync(path.join(root, e.name, 'index.html')))
                    .map((e) => e.name)
                    .sort();
                server.transformIndexHtml('/', page(items)).then(
                    (html) => {
                        res.setHeader('Content-Type', 'text/html');
                        res.end(html);
                    },
                    (err) => next(err),
                );
            });
        },
    };
}

function shouldPoll(root) {
    const env = process.env.BSDEV_PROTO_POLL;
    if (env === '0' || env === '1') return env === '1';
    try {
        const hostRepos = fs.realpathSync(path.join(os.homedir(), 'host-repos'));
        return root === hostRepos || root.startsWith(`${hostRepos}${path.sep}`);
    } catch {
        return false;
    }
}

async function serve(repo) {
    const { createServer } = await import('vite');
    const { default: react } = await import('@vitejs/plugin-react');
    const { default: tailwindcss } = await import('@tailwindcss/vite');

    const p = paths(repo);
    const root = fs.realpathSync(path.join(repo, PROTOTYPES));
    const cacheDir = path.join(p.cache, 'vite');
    const { alias, plugins } = runtimeResolution();
    const poll = shouldPoll(root);

    const server = await createServer({
        configFile: false,
        envDir: false,
        root,
        cacheDir,
        clearScreen: false,
        appType: 'mpa',
        plugins: [...plugins, indexPlugin(root), react(), tailwindcss()],
        resolve: { alias },
        server: {
            host: '127.0.0.1',
            port: DEFAULT_PORT,
            strictPort: false,
            fs: { allow: [root, RUNTIME, cacheDir] },
            watch: poll ? { usePolling: true, interval: 200 } : {},
        },
    });
    await server.listen();
    const port = server.httpServer.address().port;

    const state = { pid: process.pid, port, root, repo, poll };
    fs.writeFileSync(p.state, JSON.stringify(state, null, 2));
    console.log(`bsdev-proto: serving ${root} on ${url(port)}${poll ? ' (polling)' : ''}`);

    let closing = false;
    const shutdown = async () => {
        if (closing) return;
        closing = true;
        try {
            const current = JSON.parse(fs.readFileSync(p.state, 'utf8'));
            if (current.pid === process.pid) fs.rmSync(p.state, { force: true });
        } catch {}
        await server.close().catch(() => {});
        process.exit(0);
    };
    for (const sig of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(sig, shutdown);
}

async function main() {
    let parsed;
    try {
        parsed = parseArgs({
            allowPositionals: true,
            options: {
                'no-open': { type: 'boolean' },
                out: { type: 'string' },
                viewport: { type: 'string' },
                'full-page': { type: 'boolean' },
                help: { type: 'boolean', short: 'h' },
            },
        });
    } catch (err) {
        fail(`${err.message}\n\n${USAGE}`);
    }
    const { values: opts, positionals } = parsed;
    const [command, ...rest] = positionals;

    if (opts.help || !command || command === 'help') {
        console.log(USAGE);
        return;
    }
    switch (command) {
        case 'start':
            return start(repoRoot(rest[0]), opts);
        case 'stop':
            return stop(repoRoot(rest[0]));
        case 'status':
            return status();
        case 'url':
            return printUrl(repoRoot(rest[0]));
        case 'logs':
            return logs(repoRoot(rest[0]));
        case 'shot':
            return shot(rest[0], repoRoot(rest[1]), opts);
        case 'serve':
            return serve(rest[0]);
        default:
            fail(`unknown command "${command}"\n\n${USAGE}`);
    }
}

main().catch((err) => fail(err?.stack || String(err)));
