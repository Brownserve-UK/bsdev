# syntax=docker/dockerfile:1
#
# bsdev: a fully featured Arch dev container. It runs sshd as PID 1 so it
# stays up permanently. ALL tooling lives here (gh, chezmoi, git, fish, Claude
# Code, etc etc); user-specific provisioning/config can be done afterward by chezmoi
# if desired.
#
FROM archlinux:latest

ARG BSDEV_USER=bsdev
ENV BSDEV_USER=${BSDEV_USER}

ENV ANDROID_SDK_ROOT=/opt/android-sdk
ENV ANDROID_HOME=${ANDROID_SDK_ROOT}
ENV JAVA_HOME=/usr/lib/jvm/java-21-openjdk
ENV PATH=${ANDROID_SDK_ROOT}/cmdline-tools/latest/bin:${ANDROID_SDK_ROOT}/platform-tools:${PATH}

# Refresh the keyring first (a pulled base image can carry a stale
# archlinux-keyring, which makes later package signatures fail), then install
# the full toolset. openssh -> sshd; sudo is not guaranteed by base-devel;
# jdk21-openjdk -> Gradle/AGP and sdkmanager both need a JDK.
RUN pacman -Sy --needed --noconfirm archlinux-keyring \
    && pacman -Su --needed --noconfirm \
        base-devel git openssh sudo nano curl \
        github-cli fish fastfetch wget chezmoi \
        rustup nodejs npm \
        jdk21-openjdk \
    && pacman -Scc --noconfirm

# Create the login user in wheel with validated passwordless sudo (a typo can't
# lock us out), with fish as the login shell.
RUN useradd -m -G wheel,users -s /usr/bin/fish "${BSDEV_USER}" \
    && printf '%%wheel ALL=(ALL:ALL) NOPASSWD: ALL\n' > /etc/sudoers.d/10-wheel-nopasswd \
    && chmod 0440 /etc/sudoers.d/10-wheel-nopasswd \
    && visudo -cf /etc/sudoers.d/10-wheel-nopasswd

# The marker chezmoi and the shell rc files test for to detect the container.
RUN touch /etc/bsdev-container

# AUR tooling, Claude Code + codex, installed as the login user (makepkg refuses to run
# as root; the native Claude installer drops a self-updating binary in ~/.local/bin)
# Which topgrade can handle upgrading.
# Fetches from aur.archlinux.org are occasionally flaky in CI (transient TLS
# EOFs), so retry the network-bound steps; --needed keeps retries idempotent.
USER ${BSDEV_USER}
RUN export HOME="/home/${BSDEV_USER}"; \
    retry() { for i in 1 2 3; do "$@" && return 0; echo ">> attempt $i failed: $* - retrying..." >&2; sleep 5; done; return 1; }; \
    tmp="$(mktemp -d)" \
    && retry git clone https://aur.archlinux.org/yay-bin.git "$tmp" \
    && ( cd "$tmp" && makepkg -si --noconfirm ) \
    && rm -rf "$tmp" \
    && retry yay -S --needed --noconfirm oh-my-posh-bin tenv-bin topgrade-bin powershell-bin \
        android-sdk-cmdline-tools-latest android-sdk-platform-tools android-sdk-build-tools \
    && retry sh -c "curl -fsSL https://claude.ai/install.sh | bash" \
    && retry sh -c "curl -fsSL https://chatgpt.com/codex/install.sh | sh"

# rustup itself comes from the official repos above; install+select the stable
# toolchain (rustc, cargo, rustfmt, clippy) by default so it's ready out of the
# box. Retried like the AUR fetches above - it also pulls a fair amount over the
# network and can hit the same transient CI flakiness.
RUN export HOME="/home/${BSDEV_USER}"; \
    retry() { for i in 1 2 3; do "$@" && return 0; echo ">> attempt $i failed: $* - retrying..." >&2; sleep 5; done; return 1; }; \
    retry rustup default stable
USER root

# sshd: generate host keys and allow key-based auth only.
RUN ssh-keygen -A \
    && sed -i \
        -e 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' \
        -e 's/^#\?PubkeyAuthentication.*/PubkeyAuthentication yes/' \
        -e 's/^#\?PermitRootLogin.*/PermitRootLogin no/' \
        /etc/ssh/sshd_config

# Accept the Android SDK licenses (AGP/Gradle check for these before building)
# and hand ownership to ${BSDEV_USER} so they can add further SDK components
# later (sdkmanager --install ...) without needing sudo.
RUN yes | ${ANDROID_SDK_ROOT}/cmdline-tools/latest/bin/sdkmanager --licenses > /dev/null \
    && chown -R ${BSDEV_USER}:${BSDEV_USER} ${ANDROID_SDK_ROOT}

COPY bsdev-entrypoint.sh /usr/local/bin/bsdev-entrypoint.sh
# In-container `code` shim: opens paths in the host VSCode over the bsdev reverse
# channel (see image/code). VSCode's own remote-cli takes PATH precedence inside
# a Remote-SSH integrated terminal, so this only applies to bare bsdev sessions.
COPY code /usr/local/bin/code
RUN chmod 0755 /usr/local/bin/bsdev-entrypoint.sh /usr/local/bin/code

# Put rustup's cargo/rustc shims on PATH for fish (the container's login shell).
COPY fish-cargo-path.fish /etc/fish/conf.d/bsdev-cargo-path.fish
RUN chmod 0644 /etc/fish/conf.d/bsdev-cargo-path.fish

# sshd doesn't propagate the image's ENV into login shells, so the Android SDK
# env + PATH need setting again here (same reason as the cargo snippet above).
COPY fish-android-path.fish /etc/fish/conf.d/bsdev-android-path.fish
RUN chmod 0644 /etc/fish/conf.d/bsdev-android-path.fish

# Tell VSCode Dev Containers to attach as `bsdev`. The container's default user is
# root (so sshd can run as PID 1), but "Attach to Running Container" would then run
# as root and read /root/.gitconfig instead of bsdev's chezmoi config. This label
# makes VSCode attach as bsdev, matching the `bsdev` ssh session.
LABEL devcontainer.metadata='[{"remoteUser":"bsdev"}]'

EXPOSE 22
ENTRYPOINT ["/usr/local/bin/bsdev-entrypoint.sh"]
