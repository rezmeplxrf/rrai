#!/usr/bin/env bash
set -euo pipefail

# RRAI — Rust Remote AI Installer
#
# Usage:
#   Local install (on the target machine):
#     ./scripts/install.sh                           # Install latest release
#     ./scripts/install.sh v0.2.0                    # Install specific version
#     ./scripts/install.sh --from-source             # Build from source
#
#   Remote install (from your dev machine via SSH):
#     ./scripts/install.sh --remote user@host                    # Install on remote VM
#     ./scripts/install.sh --remote user@host v0.2.0             # Specific version
#     ./scripts/install.sh --remote user@host --env ~/my/.env    # Custom .env path
#     ./scripts/install.sh --remote user@host --no-env           # Skip .env (configure later)
#
#   Remote install requires a local .env (default: ./.env in script directory).
#   Fails if not found — use --env to specify or --no-env to skip.

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
err()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }

REPO="rezmeplxrf/rrai"
SCRIPT_URL="https://raw.githubusercontent.com/$REPO/main/scripts/install.sh"

# Portable sha256: macOS has shasum, Linux has sha256sum
sha256() {
    if command -v sha256sum &>/dev/null; then
        sha256sum "$@"
    elif command -v shasum &>/dev/null; then
        shasum -a 256 "$@"
    else
        err "No sha256 tool found (need sha256sum or shasum)"
        return 1
    fi
}

# ============================================================
# Remote mode: SSH into the target and run installer there
# ============================================================
if [[ "${1:-}" == "--remote" ]]; then
    shift
    SSH_TARGET="${1:?Usage: ./scripts/install.sh --remote user@host [version] [--env path] [--no-env]}"
    shift

    REMOTE_VERSION="latest"
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    LOCAL_ENV="$SCRIPT_DIR/../.env"
    SKIP_ENV=false

    # Parse remaining args: [version] [--env path] [--no-env]
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --env)
                LOCAL_ENV="${2:?--env requires a path}"
                shift 2
                ;;
            --no-env)
                SKIP_ENV=true
                shift
                ;;
            *)
                REMOTE_VERSION="$1"
                shift
                ;;
        esac
    done

    echo ""
    echo "=============================="
    echo "  RRAI — Remote Deploy"
    echo "  Target: $SSH_TARGET"
    echo "=============================="
    echo ""

    # Test SSH connection
    info "Testing SSH connection..."
    if ! ssh -o ConnectTimeout=10 -o BatchMode=yes "$SSH_TARGET" "echo ok" &>/dev/null; then
        err "Cannot connect to $SSH_TARGET"
        err "Ensure SSH key auth is configured (ssh-copy-id $SSH_TARGET)"
        exit 1
    fi
    ok "SSH connection verified"

    # Push .env to remote
    if $SKIP_ENV; then
        warn "Skipping .env — remote will need manual configuration"
    elif [ -f "$LOCAL_ENV" ] && [ -s "$LOCAL_ENV" ]; then
        info "Pushing $LOCAL_ENV to $SSH_TARGET..."
        ssh "$SSH_TARGET" "mkdir -p ~/.local/share/rrai"
        scp -q "$LOCAL_ENV" "$SSH_TARGET:~/.local/share/rrai/.env"
        ok "Config deployed to remote"
    else
        err "No .env found at $LOCAL_ENV"
        err "The bot requires a configured .env to run."
        err ""
        err "Options:"
        err "  --env /path/to/.env    Specify .env location"
        err "  --no-env               Skip (configure manually on remote)"
        exit 1
    fi

    # Run installer on remote non-interactively (auto-setup systemd)
    info "Running installer on $SSH_TARGET..."
    ssh -t "$SSH_TARGET" "bash -c '
        set -euo pipefail
        INSTALLER=\$(mktemp)
        curl -fsSL -o \"\$INSTALLER\" \"$SCRIPT_URL\"
        chmod +x \"\$INSTALLER\"
        RRAI_AUTO_SYSTEMD=1 bash \"\$INSTALLER\" $REMOTE_VERSION
        rm -f \"\$INSTALLER\"
    '"

    echo ""
    ok "Remote deploy complete!"
    echo ""
    echo "Manage the service:"
    echo "  ssh $SSH_TARGET systemctl --user status rrai"
    echo "  ssh $SSH_TARGET journalctl --user -u rrai -f"
    echo ""
    exit 0
fi

# ============================================================
# Local mode: install on this machine
# ============================================================

INSTALL_DIR="${RRAI_INSTALL_DIR:-$HOME/.local/bin}"
DATA_DIR="${RRAI_DATA_DIR:-$HOME/.local/share/rrai}"
VERSION="${1:-latest}"
FROM_SOURCE=false

if [[ "$VERSION" == "--from-source" ]]; then
    FROM_SOURCE=true
    VERSION="latest"
fi

echo ""
echo "=============================="
echo "  RRAI — Rust Remote AI"
echo "  Installer"
echo "=============================="
echo ""

# --- Detect architecture ---
detect_arch() {
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) err "Unsupported architecture: $arch"; exit 1 ;;
    esac
}

ARCH=$(detect_arch)
info "Detected architecture: $ARCH"

# --- Check/install Claude Code CLI ---
if ! command -v claude &>/dev/null; then
    if command -v npm &>/dev/null; then
        info "Installing Claude Code CLI..."
        npm install -g @anthropic-ai/claude-code
        ok "Claude Code CLI installed"
    elif command -v node &>/dev/null; then
        warn "npm not found. Please install Claude Code CLI manually:"
        warn "  npm install -g @anthropic-ai/claude-code"
    else
        warn "Node.js not found. Claude Code CLI is required at runtime."
        warn "Install Node.js 18+ first, then run:"
        warn "  npm install -g @anthropic-ai/claude-code"
    fi
else
    ok "Claude Code CLI found: $(claude --version 2>/dev/null || echo 'installed')"
fi

# --- Install binary ---
if $FROM_SOURCE; then
    info "Building from source..."

    for cmd in cargo git; do
        if ! command -v "$cmd" &>/dev/null; then
            if [[ "$cmd" == "cargo" ]]; then
                info "Rust not found. Installing via rustup..."
                curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
                source "$HOME/.cargo/env"
                ok "Rust installed"
            else
                err "$cmd is required for --from-source. Install it first."
                exit 1
            fi
        fi
    done
    ok "Rust found: $(rustc --version)"

    BUILD_TMPDIR=$(mktemp -d)
    trap 'rm -rf "$BUILD_TMPDIR"' EXIT
    info "Cloning repository..."
    git clone --depth 1 "https://github.com/$REPO.git" "$BUILD_TMPDIR/rrai"
    cd "$BUILD_TMPDIR/rrai"
    info "Building (release mode)..."
    cargo build --release
    mkdir -p "$INSTALL_DIR"
    cp target/release/rrai "$INSTALL_DIR/rrai"
    chmod +x "$INSTALL_DIR/rrai"
    ok "Built and installed to $INSTALL_DIR/rrai"
else
    # --- Download pre-built binary ---
    if [[ "$VERSION" == "latest" ]]; then
        info "Fetching latest release..."
        DOWNLOAD_URL="https://github.com/$REPO/releases/latest/download/rrai-linux-${ARCH}"
        CHECKSUM_URL="https://github.com/$REPO/releases/latest/download/checksums-sha256.txt"
    else
        info "Fetching release $VERSION..."
        DOWNLOAD_URL="https://github.com/$REPO/releases/download/${VERSION}/rrai-linux-${ARCH}"
        CHECKSUM_URL="https://github.com/$REPO/releases/download/${VERSION}/checksums-sha256.txt"
    fi

    mkdir -p "$INSTALL_DIR"
    BINARY_PATH="$INSTALL_DIR/rrai"

    info "Downloading rrai-linux-${ARCH}..."
    if ! curl -fSL --progress-bar -o "$BINARY_PATH" "$DOWNLOAD_URL"; then
        err "Download failed. Check that the release exists:"
        err "  https://github.com/$REPO/releases"
        exit 1
    fi
    chmod +x "$BINARY_PATH"

    # Verify checksum
    TMPCHECK=$(mktemp)
    if curl -fsSL -o "$TMPCHECK" "$CHECKSUM_URL" 2>/dev/null; then
        EXPECTED=$(grep "rrai-linux-${ARCH}" "$TMPCHECK" | awk '{print $1}')
        ACTUAL=$(sha256 "$BINARY_PATH" | awk '{print $1}')
        rm -f "$TMPCHECK"
        if [[ "$EXPECTED" == "$ACTUAL" ]]; then
            ok "Checksum verified"
        else
            err "Checksum mismatch! Expected: $EXPECTED  Got: $ACTUAL"
            rm -f "$BINARY_PATH"
            exit 1
        fi
    else
        rm -f "$TMPCHECK"
        warn "Could not download checksums — skipping verification"
    fi

    ok "Installed to $BINARY_PATH"
fi

# --- Ensure install dir is on PATH ---
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    warn "$INSTALL_DIR is not on PATH"
    SHELL_RC=""
    if [ -f "$HOME/.bashrc" ]; then SHELL_RC="$HOME/.bashrc";
    elif [ -f "$HOME/.zshrc" ]; then SHELL_RC="$HOME/.zshrc";
    elif [ -f "$HOME/.profile" ]; then SHELL_RC="$HOME/.profile"; fi

    if [ -n "$SHELL_RC" ]; then
        if ! grep -q "$INSTALL_DIR" "$SHELL_RC" 2>/dev/null; then
            echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$SHELL_RC"
            ok "Added $INSTALL_DIR to PATH in $SHELL_RC"
        fi
        info "Run: source $SHELL_RC  (or open a new terminal)"
    else
        warn "Add this to your shell config: export PATH=\"$INSTALL_DIR:\$PATH\""
    fi
fi

# --- Set up data directory with .env ---
mkdir -p "$DATA_DIR"
if [ ! -f "$DATA_DIR/.env" ]; then
    cat > "$DATA_DIR/.env" <<'ENVFILE'
# RRAI Configuration
# See: https://github.com/rezmeplxrf/rrai

# Discord Bot Token (from https://discord.com/developers/applications)
DISCORD_BOT_TOKEN=

# Discord Server (Guild) ID
DISCORD_GUILD_ID=

# Comma-separated Discord User IDs allowed to use the bot
ALLOWED_USER_IDS=

# Base directory for auto-created project folders
BASE_PROJECT_DIR=~/projects

# Max messages per user per minute (default: 10)
# RATE_LIMIT_PER_MINUTE=10
ENVFILE
    warn "Created $DATA_DIR/.env — edit it with your settings"
else
    ok "Config exists: $DATA_DIR/.env"
fi

# --- Systemd service setup ---
# RRAI_AUTO_SYSTEMD=1 skips the prompt (used by --remote mode)
setup_service="n"
if [[ "${RRAI_AUTO_SYSTEMD:-}" == "1" ]]; then
    setup_service="y"
else
    read -rp "Set up systemd service for auto-start on boot? [y/N] " setup_service
fi

if [[ "${setup_service,,}" == "y" ]]; then
    SERVICE_DIR="$HOME/.config/systemd/user"
    SERVICE_FILE="$SERVICE_DIR/rrai.service"
    mkdir -p "$SERVICE_DIR"

    cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=RRAI - Rust Remote AI
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=$DATA_DIR
ExecStart=$INSTALL_DIR/rrai
Restart=on-failure
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF

    systemctl --user daemon-reload
    systemctl --user enable rrai.service
    ok "Systemd service installed and enabled"

    # Enable lingering so user services run without an active login session
    if command -v loginctl &>/dev/null; then
        loginctl enable-linger "$USER" 2>/dev/null && \
            ok "Lingering enabled — service will run even when logged out" || \
            warn "Could not enable lingering. Run: sudo loginctl enable-linger $USER"
    fi

    # Auto-start if .env looks configured, skip prompt in auto mode
    if [[ "${RRAI_AUTO_SYSTEMD:-}" == "1" ]]; then
        if ! grep -q '^DISCORD_BOT_TOKEN=$' "$DATA_DIR/.env" 2>/dev/null; then
            systemctl --user start rrai.service
            ok "Service started"
        else
            warn ".env not configured — start manually after editing:"
            warn "  systemctl --user start rrai"
        fi
    else
        echo ""
        read -rp "Start the service now? [y/N] " start_now
        if [[ "${start_now,,}" == "y" ]]; then
            if grep -q '^DISCORD_BOT_TOKEN=$' "$DATA_DIR/.env" 2>/dev/null; then
                warn "Please edit $DATA_DIR/.env first, then run:"
                warn "  systemctl --user start rrai"
            else
                systemctl --user start rrai.service
                ok "Service started"
                info "Check status: systemctl --user status rrai"
                info "View logs:    journalctl --user -u rrai -f"
            fi
        fi
    fi
else
    info "Skipping systemd setup."
fi

echo ""
echo "=============================="
echo "  Installation Complete!"
echo "=============================="
echo ""
echo "Next steps:"
if [[ "${setup_service,,}" == "y" ]]; then
    if grep -q '^DISCORD_BOT_TOKEN=$' "$DATA_DIR/.env" 2>/dev/null; then
echo "  1. Edit $DATA_DIR/.env with your Discord bot token and settings"
echo "  2. Start: systemctl --user start rrai"
    else
echo "  1. Check: systemctl --user status rrai"
    fi
echo "  Logs: journalctl --user -u rrai -f"
else
echo "  1. Edit $DATA_DIR/.env with your Discord bot token and settings"
echo "  2. Run:  cd $DATA_DIR && rrai"
fi
echo ""
echo "To update later:"
echo "  curl -fsSL $SCRIPT_URL | bash"
echo ""
