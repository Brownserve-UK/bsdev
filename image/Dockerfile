# syntax=docker/dockerfile:1
#
# bsdev: a batteries-included Arch dev container. It runs sshd as PID 1 so it
# stays up permanently. ALL tooling lives here (gh, chezmoi, git, fish, Claude
# Code, ...); user-specific provisioning/config is done afterward by chezmoi
# (bootstrap-bsdev.sh in the portable_config repo), not by this image.
#
# The root prep mirrors bootstrap/provision-wsl-root.sh from portable_config.
# Published to ghcr.io/brownserve-uk/bsdev by this repo's CI.
#
FROM archlinux:latest

ARG BSDEV_USER=bsdev
ENV BSDEV_USER=${BSDEV_USER}

# Refresh the keyring first (a pulled base image can carry a stale
# archlinux-keyring, which makes later package signatures fail), then install
# the full toolset. openssh -> sshd; sudo is not guaranteed by base-devel.
RUN pacman -Sy --needed --noconfirm archlinux-keyring \
    && pacman -Su --needed --noconfirm \
        base-devel git openssh sudo nano curl \
        github-cli fish fastfetch wget chezmoi \
    && pacman -Scc --noconfirm

# Create the login user in wheel with validated passwordless sudo (a typo can't
# lock us out), with fish as the login shell.
RUN useradd -m -G wheel,users -s /usr/bin/fish "${BSDEV_USER}" \
    && printf '%%wheel ALL=(ALL:ALL) NOPASSWD: ALL\n' > /etc/sudoers.d/10-wheel-nopasswd \
    && chmod 0440 /etc/sudoers.d/10-wheel-nopasswd \
    && visudo -cf /etc/sudoers.d/10-wheel-nopasswd

# The marker chezmoi and the shell rc files test for to detect the container.
RUN touch /etc/bsdev-container

# AUR tooling + Claude Code, installed as the login user (makepkg refuses to run
# as root; the native Claude installer drops a self-updating binary in ~/.local/bin).
USER ${BSDEV_USER}
RUN export HOME="/home/${BSDEV_USER}" \
    && tmp="$(mktemp -d)" \
    && git clone https://aur.archlinux.org/yay-bin.git "$tmp" \
    && (cd "$tmp" && makepkg -si --noconfirm) \
    && rm -rf "$tmp" \
    && yay -S --needed --noconfirm oh-my-posh-bin tenv-bin topgrade-bin \
    && curl -fsSL https://claude.ai/install.sh | bash
USER root

# sshd: generate host keys and allow key-based auth only.
RUN ssh-keygen -A \
    && sed -i \
        -e 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' \
        -e 's/^#\?PubkeyAuthentication.*/PubkeyAuthentication yes/' \
        -e 's/^#\?PermitRootLogin.*/PermitRootLogin no/' \
        /etc/ssh/sshd_config

COPY bsdev-entrypoint.sh /usr/local/bin/bsdev-entrypoint.sh
RUN chmod 0755 /usr/local/bin/bsdev-entrypoint.sh

EXPOSE 22
ENTRYPOINT ["/usr/local/bin/bsdev-entrypoint.sh"]
