#!/usr/bin/env bash
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  echo "Run this script as root." >&2
  exit 1
fi

TRENCH_TOOLS_REPO_URL="${TRENCH_TOOLS_REPO_URL:-${LAUNCHDECK_REPO_URL:-https://github.com/0xD3bt/Trench-Tools.git}}"
TRENCH_TOOLS_REPO_BRANCH="${TRENCH_TOOLS_REPO_BRANCH:-${LAUNCHDECK_REPO_BRANCH:-master}}"
TRENCH_TOOLS_DIR="${TRENCH_TOOLS_DIR:-${LAUNCHDECK_DIR:-/root/trench.tools}}"
TRENCH_TOOLS_SERVICE_NAME="${TRENCH_TOOLS_SERVICE_NAME:-${LAUNCHDECK_SERVICE_NAME:-trenchtools}}"
NODE_MAJOR="${NODE_MAJOR:-20}"

install_base_packages() {
  apt-get update
  apt-get install -y \
    ca-certificates \
    curl \
    git \
    wget \
    unzip \
    tmux \
    htop \
    jq \
    lsof \
    iproute2 \
    build-essential \
    pkg-config \
    libssl-dev \
    ufw \
    fail2ban \
    gnupg
}

install_rust() {
  if [[ ! -x /root/.cargo/bin/rustup ]]; then
    curl https://sh.rustup.rs -sSf | sh -s -- -y
  fi

  if ! grep -q '.cargo/env' /root/.bashrc 2>/dev/null; then
    echo '. "$HOME/.cargo/env"' >> /root/.bashrc
  fi

  # shellcheck disable=SC1091
  source /root/.cargo/env
  rustup default stable
}

install_node() {
  if [[ ! -f /etc/apt/keyrings/nodesource.gpg ]]; then
    mkdir -p /etc/apt/keyrings
    curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
      | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg
  fi

  echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" \
    > /etc/apt/sources.list.d/nodesource.list

  apt-get update
  apt-get install -y nodejs
}

sync_repo() {
  mkdir -p "$(dirname "$TRENCH_TOOLS_DIR")"

  if [[ ! -d "$TRENCH_TOOLS_DIR/.git" ]]; then
    git clone --branch "$TRENCH_TOOLS_REPO_BRANCH" "$TRENCH_TOOLS_REPO_URL" "$TRENCH_TOOLS_DIR"
  else
    git -C "$TRENCH_TOOLS_DIR" fetch origin "$TRENCH_TOOLS_REPO_BRANCH"
    git -C "$TRENCH_TOOLS_DIR" checkout "$TRENCH_TOOLS_REPO_BRANCH"
    git -C "$TRENCH_TOOLS_DIR" pull --ff-only origin "$TRENCH_TOOLS_REPO_BRANCH"
  fi

  cd "$TRENCH_TOOLS_DIR"
  npm install
  chmod +x "$TRENCH_TOOLS_DIR"/trench-tools-*.sh

  if [[ ! -f "$TRENCH_TOOLS_DIR/.env" ]]; then
    cp "$TRENCH_TOOLS_DIR/.env.example" "$TRENCH_TOOLS_DIR/.env"
  fi
}

write_systemd_service() {
  cat >/etc/systemd/system/${TRENCH_TOOLS_SERVICE_NAME}.service <<EOF
[Unit]
Description=Trench Tools runtime
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=${TRENCH_TOOLS_DIR}
Environment=HOME=/root
Environment=CARGO_HOME=/root/.cargo
Environment=RUSTUP_HOME=/root/.rustup
Environment=PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
ExecStart=/usr/bin/env bash -lc 'cd "${TRENCH_TOOLS_DIR}" && npm start'
ExecStop=/usr/bin/env bash -lc 'cd "${TRENCH_TOOLS_DIR}" && npm stop'
ExecReload=/usr/bin/env bash -lc 'cd "${TRENCH_TOOLS_DIR}" && npm restart'
TimeoutStartSec=1800
TimeoutStopSec=180

[Install]
WantedBy=multi-user.target
EOF

  systemctl daemon-reload
  systemctl enable "${TRENCH_TOOLS_SERVICE_NAME}.service"
}

configure_host_security() {
  systemctl enable fail2ban
  systemctl restart fail2ban

  ufw allow OpenSSH
  ufw --force enable
}

start_trench_tools() {
  systemctl restart "${TRENCH_TOOLS_SERVICE_NAME}.service"
}

print_next_steps() {
  cat <<EOF

Trench Tools bootstrap complete.

Project path:
  ${TRENCH_TOOLS_DIR}

Service commands:
  systemctl status ${TRENCH_TOOLS_SERVICE_NAME}
  systemctl restart ${TRENCH_TOOLS_SERVICE_NAME}
  journalctl -u ${TRENCH_TOOLS_SERVICE_NAME} -n 100 --no-pager

Default local hosts on the VPS:
  execution-engine:          http://127.0.0.1:8788
  launchdeck-engine:         http://127.0.0.1:8789
  launchdeck-follow-daemon:  http://127.0.0.1:8790

Shared auth token file:
  ${TRENCH_TOOLS_DIR}/.local/trench-tools/default-engine-token.txt

Next steps:
  1. Edit ${TRENCH_TOOLS_DIR}/.env
  2. Restart the service:
       systemctl restart ${TRENCH_TOOLS_SERVICE_NAME}
  3. On your local computer, add this SSH config entry:
       Host Trenchtools-vps
         HostName YOUR_SERVER_IP
         User root
         IdentityFile ~/.ssh/id_ed25519
         IdentitiesOnly yes
         LocalForward 8788 127.0.0.1:8788
         LocalForward 8789 127.0.0.1:8789
         ExitOnForwardFailure yes
         ServerAliveInterval 30
  4. Connect with:
       ssh Trenchtools-vps
  5. Keep that SSH session open and use these local URLs in your browser/extension:
       Execution host:  http://127.0.0.1:8788
       LaunchDeck host: http://127.0.0.1:8789

Manual fallback if you did not add the SSH config:
  ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP

Keep the raw local hosts private. Use SSH tunnels unless you intentionally add your own HTTPS reverse proxy and access controls.
EOF
}

install_base_packages
install_rust
install_node
sync_repo
write_systemd_service
configure_host_security
start_trench_tools
print_next_steps
