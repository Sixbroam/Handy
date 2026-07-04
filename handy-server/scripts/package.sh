#!/bin/bash
# package.sh — produit dist/handy-server + dist/lib/*.so autoporteur (rpath $ORIGIN/lib)
# Usage : ./scripts/package.sh [--release|--debug]
set -euo pipefail

PROFILE="${1:---release}"
case "$PROFILE" in
    --release) PROFILE_DIR="release" ;;
    --debug|*)  PROFILE_DIR="debug" ;;
esac

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"

echo "==> Packaging handy-server ($PROFILE)..."

# Build with rpath so the binary finds libs at $ORIGIN/lib without LD_LIBRARY_PATH
export RUSTFLAGS="-C link-args=-Wl,-rpath,\$ORIGIN/lib"
cargo build "$PROFILE" --manifest-path "$ROOT_DIR/Cargo.toml" 2>&1 | grep -v "^$" || true

BINARY="$ROOT_DIR/target/$PROFILE_DIR/handy-server"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: Binary not found at $BINARY" >&2
    exit 1
fi

# Clean previous dist
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/lib"

# Copy binary
cp "$BINARY" "$DIST_DIR/handy-server"
chmod +x "$DIST_DIR/handy-server"

echo "==> Finding native libraries..."

# Find transcribe-cpp-sys build artifacts
BUILD_DIR=$(find "$ROOT_DIR/target/$PROFILE_DIR/build" -type d -name "transcribe-cpp-sys-*" 2>/dev/null | head -1)
if [ -n "$BUILD_DIR" ]; then
    LIB_SRC="$BUILD_DIR/out/lib"
    if [ -d "$LIB_SRC" ]; then
        # Copy .so files from transcribe-cpp-sys build output
        cp "$LIB_SRC"/*.so* "$DIST_DIR/lib/" 2>/dev/null || true
        echo "  Copied libs from $LIB_SRC"
    fi

    # Also check ../build/src and ../build/ggml/src for additional libs (F6)
    for SUBDIR in "src" "ggml/src"; do
        EXTRA_LIB="$BUILD_DIR/../build/$SUBDIR"
        if [ -d "$EXTRA_LIB" ]; then
            cp "$EXTRA_LIB"/*.so* "$DIST_DIR/lib/" 2>/dev/null || true
        fi
    done
else
    echo "WARNING: transcribe-cpp-sys build dir not found" >&2
fi

# Check if ort (onnxruntime) requires libonnxruntime at runtime
# ldd will show "not found" for missing deps
MISSING=$(ldd "$DIST_DIR/handy-server" 2>/dev/null | grep "not found" | awk '{print $3}' || true)
if [ -n "$MISSING" ]; then
    echo "WARNING: Missing runtime dependencies detected:" >&2
    echo "$MISSING" >&2
    # Try to find and copy them from the build tree
    for DEP in $MISSING; do
        FOUND=$(find "$ROOT_DIR/target/$PROFILE_DIR/build" -name "$(basename "$DEP")" -type f 2>/dev/null | head -1)
        if [ -n "$FOUND" ]; then
            cp "$FOUND" "$DIST_DIR/lib/"
            echo "  Copied missing: $DEP from $FOUND"
        else
            echo "  Could not find: $DEP (may need manual install)" >&2
        fi
    done
fi

# Verify rpath is set
RPATH_OUTPUT=$(readelf -d "$DIST_DIR/handy-server" 2>/dev/null | grep RPATH || true)
if [ -z "$RPATH_OUTPUT" ]; then
    # Fallback: patchelf if available
    if command -v patchelf &>/dev/null; then
        echo "==> Patching rpath with patchelf..."
        patchelf --set-rpath '$ORIGIN/lib' "$DIST_DIR/handy-server"
    else
        echo "WARNING: rpath not set and patchelf not available. Install patchelf or set LD_LIBRARY_PATH." >&2
    fi
fi

# Count libs
LIB_COUNT=$(find "$DIST_DIR/lib" -name "*.so*" -type f 2>/dev/null | wc -l)
echo ""
echo "==> Package ready at $DIST_DIR/"
echo "    Binary: $(du -h "$DIST_DIR/handy-server" | cut -f1)"
echo "    Libraries: $LIB_COUNT .so files in dist/lib/"
ls -la "$DIST_DIR/lib/" 2>/dev/null || echo "    (no libs copied)"
echo ""
echo "Test : cp -r $DIST_DIR /tmp/ && /tmp/dist/handy-server --help"
