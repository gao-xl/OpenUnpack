#!/usr/bin/env bash
# ==============================================================================
# OpenUnpack Cross-Platform Headless & CLI Build Script (Linux / macOS)
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "=================================================="
echo " Building OpenUnpack Headless Daemon & CLI Tools"
echo " Target OS: $(uname -s) ($(uname -m))"
echo "=================================================="

cd "${ROOT_DIR}"

BUILD_MODE="--release"
TARGET_DIR="target/release"

echo "[1/3] Compiling Rust binaries (openunpack-desktop --headless & openunpack CLI)..."
cargo build ${BUILD_MODE} -p openunpack-desktop -p openunpack-cli

OUT_DIR="${ROOT_DIR}/artifacts/headless-server"
mkdir -p "${OUT_DIR}"

echo "[2/3] Collecting artifacts into ${OUT_DIR}..."
cp "${TARGET_DIR}/openunpack-desktop" "${OUT_DIR}/openunpack-daemon"
cp "${TARGET_DIR}/openunpack" "${OUT_DIR}/openunpack"
chmod +x "${OUT_DIR}/openunpack-daemon" "${OUT_DIR}/openunpack"

if [ -d "${ROOT_DIR}/scripts/integrations" ]; then
    cp -r "${ROOT_DIR}/scripts/integrations" "${OUT_DIR}/"
fi

echo "[3/3] Build succeeded!"
echo "=================================================="
echo " Output directory: ${OUT_DIR}"
echo " Start daemon:     ${OUT_DIR}/openunpack-daemon --headless --port 18420"
echo " CLI status check: ${OUT_DIR}/openunpack status"
echo "=================================================="
