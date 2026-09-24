# bsdev-proto

Serves design prototypes from `<repo>/.agents/prototypes` with live reload (Vite + React Fast
Refresh), bound to `127.0.0.1` inside the container. Baked into the image at `/opt/bsdev-proto`
and on PATH as `bsdev-proto`. Nothing is installed into the repo.

## Commands

```sh
bsdev-proto start [repo]        # start or reuse the server, URL on the last line of stdout
bsdev-proto stop [repo]
bsdev-proto status              # all running servers
bsdev-proto url [repo]
bsdev-proto logs [repo]         # Vite output, including compile errors
bsdev-proto shot <page> [repo] [--out file.png] [--viewport 412x915] [--full-page]
```

`[repo]` defaults to the git root of the current directory. The default port is 5199 (the next
free port is used if taken). State, logs, the Vite cache and screenshots live in
`~/.cache/bsdev-proto`.

Reaching it from the host:

- VSCode terminal: the port is forwarded automatically and `start` opens the host browser
  (`--no-open` to skip).
- Bare `bsdev` session: run `bsdev forward <port>` on the host (`start` prints the command).

File watching uses polling under `~/host-repos` (bind mounts don't deliver inotify events
reliably). Force it with `BSDEV_PROTO_POLL=1` or `0`.

## Prototype layout

```plain
.agents/prototypes/
  <name>/
    index.html   # <div id="root"></div><script type="module" src="./main.tsx"></script>
    main.tsx
    style.css    # @import "tailwindcss";
```

`/` lists every `<name>/index.html` folder unless there's a root `index.html`.

These packages can be imported directly:

| Package | Use |
|---|---|
| `react`, `react-dom` | Components (`react-dom/client` for `createRoot`) |
| `tailwindcss` | `@import "tailwindcss";` in CSS (v4, no config file) |
| `lucide-react` | Icons |
| `motion` | Animation (`import { motion } from 'motion/react'`) |
| `recharts` | Charts |
| `clsx` | Conditional class names |
| `@fontsource-variable/inter`, `@fontsource-variable/roboto-flex` | Offline fonts (`@import` in CSS or `import` in TS) |

To add a package, add it to `package.json`, regenerate `package-lock.json` and rebuild the image.
