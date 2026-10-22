#!/usr/bin/env bash
# ==============================================================================
# OpenUnpack - qBittorrent Download Completion Hook (Linux / macOS)
#
# Setup in qBittorrent:
#   1. Options -> Downloads -> "Run external program on torrent completion"
#   2. Command:
#      /path/to/qbittorrent.sh "%F" "%D" "%N" "%L"
#
# Parameters:
#   $1 (%F): Content path
#   $2 (%D): Save directory
#   $3 (%N): Torrent name
#   $4 (%L): Category
# ==============================================================================

TARGET_PATH="$1"
SAVE_DIR="$2"
TORRENT_NAME="$3"
CATEGORY="$4"

if [ -z "$TARGET_PATH" ]; then
    echo "[OpenUnpack-Hook] Error: Target path is empty." >&2
    exit 1
fi

echo "[OpenUnpack-Hook] Completed download: $TORRENT_NAME -> $TARGET_PATH"

# Check if openunpack CLI is available
if command -v openunpack >/dev/null 2>&1; then
    openunpack unpack "$TARGET_PATH"
    exit $?
fi

# Fallback: Invoke Local API via curl
PORT="${OPENUNPACK_PORT:-47980}"
TOKEN="${OPENUNPACK_TOKEN}"

# Try reading from config file if unset
if [ -z "$TOKEN" ]; then
    CONFIG_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/openunpack"
    if [ -f "$CONFIG_DIR/daemon.info" ]; then
        TOKEN=$(grep -o '"token"[[:space:]]*:[[:space:]]*"[^"]*"' "$CONFIG_DIR/daemon.info" | cut -d'"' -f4)
    fi
fi

AUTH_HEADER=()
if [ -n "$TOKEN" ]; then
    AUTH_HEADER=(-H "Authorization: Bearer $TOKEN")
fi

PAYLOAD=$(cat <<EOF
{
  "command": "add_archive",
  "args": {
    "path": "$TARGET_PATH",
    "autoStart": true
  }
}
EOF
)

curl -s -X POST "http://127.0.0.1:${PORT}/api/v1/commands" \
     -H "Content-Type: application/json" \
     "${AUTH_HEADER[@]}" \
     -d "$PAYLOAD" >/dev/null

if [ $? -eq 0 ]; then
    echo "[OpenUnpack-Hook] Successfully queued via Local API."
    exit 0
else
    echo "[OpenUnpack-Hook] Failed to dispatch to OpenUnpack Local API." >&2
    exit 1
fi
