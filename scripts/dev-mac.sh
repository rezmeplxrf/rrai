#!/usr/bin/env bash
set -euo pipefail

# RRAI — Local development server for macOS.
#
# Watches for file changes and rebuilds automatically using cargo-watch.
# Installs cargo-watch if not present.
#
# Usage:
#   ./scripts/dev-mac.sh              # Run with cargo-watch (auto-reload)
#   ./scripts/dev-mac.sh --no-watch   # Run once without watching

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
err()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_DIR"

# --- Verify running on macOS ---
if [[ "$(uname -s)" != "Darwin" ]]; then
    err "This script is intended for macOS."
    exit 1
fi

# --- Check prerequisites ---
if ! command -v cargo &>/dev/null; then
    err "Rust toolchain is required. Install: https://rustup.rs/"
    exit 1
fi

# --- Check .env ---
if [ ! -f "$PROJECT_DIR/.env" ]; then
    if [ -f "$PROJECT_DIR/.env.example" ]; then
        warn "No .env found. Copy .env.example and fill in your values:"
        warn "  cp .env.example .env"
        exit 1
    else
        warn "No .env found. The bot requires DISCORD_BOT_TOKEN and other env vars to run."
        exit 1
    fi
fi

# --- Parse args ---
NO_WATCH=false
if [[ "${1:-}" == "--no-watch" ]]; then
    NO_WATCH=true
fi

export RUST_LOG="${RUST_LOG:-info}"

if $NO_WATCH; then
    info "Running rrai (RUST_LOG=$RUST_LOG)..."
    cargo run
else
    # Ensure cargo-watch is installed
    if ! command -v cargo-watch &>/dev/null; then
        info "Installing cargo-watch..."
        cargo install cargo-watch
        ok "cargo-watch installed"
    fi

    info "Watching for changes (RUST_LOG=$RUST_LOG)..."
    info "Press Ctrl+C to stop"
    cargo watch -x run
fi
