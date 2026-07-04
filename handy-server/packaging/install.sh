#!/bin/bash
# install.sh — installe handy-server en /opt/handy-server + systemd
# Usage : sudo ./scripts/install.sh [--dist <path>]
set -euo pipefail

DIST_DIR=""
for arg in "$@"; do
    case "$arg" in
        --dist) DIST_DIR="$2"; shift 2 ;;
        *) echo "Unknown option: $arg" >&2; exit 1 ;;
    esac
done

if [ -z "$DIST_DIR" ]; then
    SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
    DIST_DIR="$SCRIPT_DIR/../dist"
fi

if [ ! -f "$DIST_DIR/handy-server" ]; then
    echo "ERROR: dist/handy-server not found. Run ./scripts/package.sh --release first." >&2
    exit 1
fi

INSTALL_DIR="/opt/handy-server"
ENV_DIR="/etc/handy-server"
UNIT_FILE="$SCRIPT_DIR/../packaging/handy-server.service"

echo "==> Installing handy-server to $INSTALL_DIR ..."

# Create directories
sudo mkdir -p "$INSTALL_DIR"
sudo mkdir -p "$ENV_DIR"

# Copy dist
sudo rm -rf "${INSTALL_DIR:?}/"*
sudo cp -r "$DIST_DIR"/* "$INSTALL_DIR/"
sudo chmod +x "$INSTALL_DIR/handy-server"

# Generate token if env file doesn't exist
if [ ! -f "$ENV_DIR/env" ]; then
    echo "==> Generating auth token..."
    TOKEN=$(openssl rand -base64 32 | tr -d '=+/')
    cat > /tmp/handy-env <<EOF
HANDY_TOKEN=$TOKEN
HANDY_MODEL=handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf
HANDY_BIND=0.0.0.0:8756
# HANDY_LANGUAGE=auto
# HANDY_DEVICE=vulkan
# HANDY_GPU_DEVICE=0
EOF
    sudo mv /tmp/handy-env "$ENV_DIR/env"
    sudo chmod 600 "$ENV_DIR/env"
    echo "  Token generated (see $ENV_DIR/env)"
else
    echo "  Reusing existing env file at $ENV_DIR/env"
fi

# Install systemd unit
if [ -f "$UNIT_FILE" ]; then
    sudo cp "$UNIT_FILE" /etc/systemd/system/handy-server.service
    echo "==> Systemd unit installed."
fi

# Reload and start
sudo systemctl daemon-reload
sudo systemctl enable handy-server
sudo systemctl restart handy-server

echo ""
echo "==> Installation complete!"
echo "    Status : sudo systemctl status handy-server"
echo "    Logs   : journalctl -u handy-server -f"
echo "    Config : sudo nano $ENV_DIR/env"
echo ""
echo "CURL test : curl http://localhost:8756/health -H \"Authorization: Bearer $(sudo grep HANDY_TOKEN "$ENV_DIR/env" | cut -d= -f2)\""
