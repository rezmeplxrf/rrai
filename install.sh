#!/usr/bin/env bash
set -euo pipefail

# RRAI — Rust Remote AI Installer (Linux)

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
err()   { echo -e "${RED}[ERROR]${NC} $*"; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo ""
echo "=============================="
echo "  RRAI — Rust Remote AI"
echo "  Linux Installer"
echo "=============================="
echo ""

# --- Check/install Rust ---
if ! command -v cargo &>/dev/null; then
    info "Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    ok "Rust installed"
else
    ok "Rust found: $(rustc --version)"
fi

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
        warn "Node.js not found. Claude Code CLI is required."
        warn "Install Node.js 20+ first, then run:"
        warn "  npm install -g @anthropic-ai/claude-code"
    fi
else
    ok "Claude Code CLI found: $(claude --version 2>/dev/null || echo 'installed')"
fi

# --- Create .env if needed ---
if [ ! -f .env ]; then
    if [ -f .env.example ]; then
        cp .env.example .env
        warn ".env created from .env.example — please edit it with your settings"
    fi
fi

# --- Build ---
info "Building RRAI (release mode)..."
cargo build --release
ok "Build complete: target/release/rrai"

echo ""
echo "=============================="
echo "  Installation Complete!"
echo "=============================="
echo ""
echo "Next steps:"
echo "  1. Edit .env with your Discord bot token and settings"
echo "  2. Run: ./target/release/rrai"
echo ""
echo "Optional: Create a systemd service for auto-start:"
echo ""
cat <<'SYSTEMD'
  [Unit]
  Description=RRAI - Rust Remote AI
  After=network.target

  [Service]
  Type=simple
  WorkingDirectory=/path/to/rrai
  ExecStart=/path/to/rrai/target/release/rrai
  Restart=on-failure
  RestartSec=10
  Environment=RUST_LOG=info

  [Install]
  WantedBy=multi-user.target
SYSTEMD
echo ""
echo "Save as: ~/.config/systemd/user/rrai.service"
echo "Enable:  systemctl --user enable --now rrai"
echo ""
