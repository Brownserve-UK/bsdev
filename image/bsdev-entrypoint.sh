#!/bin/bash
set -euo pipefail

#
# bsdev container entrypoint. Runs as PID 1: install the host's public key
# (passed by the `bsdev` launcher via $BSDEV_AUTHORIZED_KEY on first run) into
# the login user's authorized_keys, then run sshd in the foreground so the
# container stays up permanently.
#

BSDEV_USER="${BSDEV_USER:-bsdev}"
USER_HOME="$(getent passwd "$BSDEV_USER" | cut -d: -f6)"

if [ -n "${BSDEV_AUTHORIZED_KEY:-}" ]; then
    install -d -m 0700 -o "$BSDEV_USER" -g "$BSDEV_USER" "$USER_HOME/.ssh"
    ak_file="$USER_HOME/.ssh/authorized_keys"
    # Add the key once (idempotent across restarts / repeated runs).
    if ! { [ -f "$ak_file" ] && grep -qxF "$BSDEV_AUTHORIZED_KEY" "$ak_file"; }; then
        printf '%s\n' "$BSDEV_AUTHORIZED_KEY" >> "$ak_file"
    fi
    chown "$BSDEV_USER:$BSDEV_USER" "$ak_file"
    chmod 0600 "$ak_file"
fi

# Defensive: regenerate host keys if a volume mount ever hid /etc/ssh contents.
ssh-keygen -A >/dev/null 2>&1 || true

exec /usr/bin/sshd -D -e
