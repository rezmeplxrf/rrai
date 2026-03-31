#!/usr/bin/env bash
set -euo pipefail

# RRAI — Cross-compile for Linux via Docker and publish to GitHub Releases.
#
# Prerequisites:
#   - Docker with buildx
#   - gh CLI (authenticated) — only for publishing
#
# Usage:
#   ./scripts/build-release.sh              # Build + release current version from Cargo.toml
#   ./scripts/build-release.sh v0.2.0       # Build + release with explicit tag
#   ./scripts/build-release.sh --build-only # Build without publishing

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
if ! command -v docker &>/dev/null; then
    err "Docker is required. Install: https://docs.docker.com/get-docker/"
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
    "x86_64-unknown-linux-gnu"
    "aarch64-unknown-linux-gnu"
)

# --- Dockerfile for cross-compilation ---
# Uses a .dockerignore-equivalent via tar exclude to avoid sending
# target/, dist/, .git/ to the build context.
DOCKERFILE=$(cat <<'DOCKERFILE_EOF'
FROM rust:1-bookworm AS builder

ARG TARGET

RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    && rm -rf /var/lib/apt/lists/*

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

WORKDIR /build
COPY . .

RUN rustup target add "$TARGET" \
    && cargo build --release --target "$TARGET" \
    && cp "target/$TARGET/release/rrai" /rrai
DOCKERFILE_EOF
)

# Write a temporary .dockerignore to exclude heavy dirs from build context
DOCKERIGNORE="$PROJECT_DIR/.dockerignore"
DOCKERIGNORE_EXISTED=false
if [ -f "$DOCKERIGNORE" ]; then
    DOCKERIGNORE_EXISTED=true
fi
cat > "$DOCKERIGNORE" <<'EOF'
target/
dist/
.git/
data.db*
.bot.lock
EOF
trap 'if ! $DOCKERIGNORE_EXISTED; then rm -f "$DOCKERIGNORE"; fi' EXIT

# --- Build for each target ---
mkdir -p "$DIST_DIR"
rm -f "$DIST_DIR"/rrai-* "$DIST_DIR"/checksums-*.txt

for TARGET in "${TARGETS[@]}"; do
    ARCH="${TARGET%%-*}"  # x86_64 or aarch64
    OUTPUT="$DIST_DIR/rrai-linux-${ARCH}"

    info "Building for $TARGET..."

    echo "$DOCKERFILE" | docker buildx build \
        --platform "linux/${ARCH/x86_64/amd64}" \
        --build-arg "TARGET=$TARGET" \
        --output "type=local,dest=$DIST_DIR/out-${ARCH}" \
        -f - \
        "$PROJECT_DIR"

    mv "$DIST_DIR/out-${ARCH}/rrai" "$OUTPUT"
    rm -rf "$DIST_DIR/out-${ARCH}"
    chmod +x "$OUTPUT"

    SIZE=$(du -h "$OUTPUT" | cut -f1)
    ok "Built $OUTPUT ($SIZE)"
done

# --- Create checksums ---
cd "$DIST_DIR"
sha256 rrai-linux-* > checksums-sha256.txt
ok "Checksums written to dist/checksums-sha256.txt"
cat checksums-sha256.txt
cd "$PROJECT_DIR"

if $BUILD_ONLY; then
    echo ""
    ok "Build complete. Binaries in dist/"
    exit 0
fi

# --- Publish to GitHub Releases ---
info "Publishing $TAG to GitHub Releases..."

# Create tag if it doesn't exist
if ! git rev-parse "$TAG" &>/dev/null; then
    git tag -a "$TAG" -m "Release $TAG"
    git push origin "$TAG"
    ok "Created and pushed tag $TAG"
fi

RELEASE_NOTES="## RRAI $TAG

Pre-built binaries for Linux:

| File | Architecture |
|------|-------------|
| \`rrai-linux-x86_64\` | x86_64 (Intel/AMD) |
| \`rrai-linux-aarch64\` | aarch64 (ARM64) |

### Install
\`\`\`bash
curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/install.sh | bash
\`\`\`

### Verify checksums
\`\`\`bash
sha256sum -c checksums-sha256.txt
\`\`\`
"

if gh release view "$TAG" --repo "$REPO" &>/dev/null; then
    warn "Release $TAG exists — updating assets"
    gh release upload "$TAG" \
        "$DIST_DIR/rrai-linux-x86_64" \
        "$DIST_DIR/rrai-linux-aarch64" \
        "$DIST_DIR/checksums-sha256.txt" \
        --repo "$REPO" \
        --clobber
else
    gh release create "$TAG" \
        "$DIST_DIR/rrai-linux-x86_64" \
        "$DIST_DIR/rrai-linux-aarch64" \
        "$DIST_DIR/checksums-sha256.txt" \
        --repo "$REPO" \
        --title "RRAI $TAG" \
        --notes "$RELEASE_NOTES"
fi

ok "Release published: https://github.com/$REPO/releases/tag/$TAG"
