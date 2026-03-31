#!/usr/bin/env bash
set -euo pipefail

# RRAI — Build for macOS (local) and optionally publish to GitHub Releases.
#
# Prerequisites:
#   - Rust toolchain (rustup)
#   - gh CLI (authenticated) — only for publishing
#
# Usage:
#   ./scripts/build-mac.sh              # Build + release current version from Cargo.toml
#   ./scripts/build-mac.sh v0.2.0       # Build + release with explicit tag
#   ./scripts/build-mac.sh --build-only # Build without publishing

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
DIST_DIR="$PROJECT_DIR/dist"
REPO="rezmeplxrf/rrai"

cd "$PROJECT_DIR"

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

# --- Verify running on macOS ---
if [[ "$(uname -s)" != "Darwin" ]]; then
    err "This script must be run on macOS."
    exit 1
fi

# --- Parse args ---
BUILD_ONLY=false
TAG="${1:-}"
if [[ "$TAG" == "--build-only" ]]; then
    BUILD_ONLY=true
    TAG=""
fi

# --- Get version from Cargo.toml ---
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
if [ -z "$TAG" ]; then
    TAG="v$VERSION"
fi
info "Version: $VERSION  Tag: $TAG"

# --- Check prerequisites ---
if ! command -v cargo &>/dev/null; then
    err "Rust toolchain is required. Install: https://rustup.rs/"
    exit 1
fi

if ! $BUILD_ONLY && ! command -v gh &>/dev/null; then
    err "gh CLI is required for publishing. Install: https://cli.github.com/"
    exit 1
fi

# --- Check for uncommitted changes when publishing ---
if ! $BUILD_ONLY; then
    if ! git diff --quiet HEAD 2>/dev/null || [ -n "$(git status --porcelain 2>/dev/null)" ]; then
        err "Working tree has uncommitted changes. Commit or stash before releasing."
        exit 1
    fi
fi

# --- Define targets ---
TARGETS=(
    "x86_64-apple-darwin"
    "aarch64-apple-darwin"
)

# --- Build for each target ---
mkdir -p "$DIST_DIR"
rm -f "$DIST_DIR"/rrai-darwin-* "$DIST_DIR"/checksums-darwin-*.txt

for TARGET in "${TARGETS[@]}"; do
    ARCH="${TARGET%%-*}"  # x86_64 or aarch64
    OUTPUT="$DIST_DIR/rrai-darwin-${ARCH}"

    info "Building for $TARGET..."

    rustup target add "$TARGET" 2>/dev/null || true
    cargo build --release --target "$TARGET"

    cp "target/$TARGET/release/rrai" "$OUTPUT"
    chmod +x "$OUTPUT"

    SIZE=$(du -h "$OUTPUT" | cut -f1)
    ok "Built $OUTPUT ($SIZE)"
done

# --- Create checksums ---
cd "$DIST_DIR"
sha256 rrai-darwin-* > checksums-darwin-sha256.txt
ok "Checksums written to dist/checksums-darwin-sha256.txt"
cat checksums-darwin-sha256.txt
cd "$PROJECT_DIR"

if $BUILD_ONLY; then
    echo ""
    ok "Build complete. Binaries in dist/"
    exit 0
fi

# --- Publish to GitHub Releases ---
info "Publishing macOS binaries to $TAG..."

# Create tag if it doesn't exist
if ! git rev-parse "$TAG" &>/dev/null; then
    git tag -a "$TAG" -m "Release $TAG"
    git push origin "$TAG"
    ok "Created and pushed tag $TAG"
fi

# Upload assets — if release exists, add to it; otherwise create it
if gh release view "$TAG" --repo "$REPO" &>/dev/null; then
    info "Release $TAG exists — uploading macOS assets"
    gh release upload "$TAG" \
        "$DIST_DIR/rrai-darwin-x86_64" \
        "$DIST_DIR/rrai-darwin-aarch64" \
        "$DIST_DIR/checksums-darwin-sha256.txt" \
        --repo "$REPO" \
        --clobber
else
    RELEASE_NOTES="## RRAI $TAG

Pre-built binaries for macOS:

| File | Architecture |
|------|-------------|
| \`rrai-darwin-x86_64\` | x86_64 (Intel) |
| \`rrai-darwin-aarch64\` | aarch64 (Apple Silicon) |

### Verify checksums
\`\`\`bash
shasum -a 256 -c checksums-darwin-sha256.txt
\`\`\`
"

    gh release create "$TAG" \
        "$DIST_DIR/rrai-darwin-x86_64" \
        "$DIST_DIR/rrai-darwin-aarch64" \
        "$DIST_DIR/checksums-darwin-sha256.txt" \
        --repo "$REPO" \
        --title "RRAI $TAG" \
        --notes "$RELEASE_NOTES"
fi

ok "macOS binaries published to: https://github.com/$REPO/releases/tag/$TAG"
