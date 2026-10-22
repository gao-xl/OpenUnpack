#!/usr/bin/env bash
# ==============================================================================
# OpenUnpack - Transmission Torrent Completion Hook
#
# Setup in transmission settings.json:
#   "script-torrent-done-enabled": true,
#   "script-torrent-done-filename": "/path/to/transmission-done.sh"
#
# Environment variables provided by Transmission:
#   $TR_TORRENT_DIR:  Save directory
#   $TR_TORRENT_NAME: Torrent name / subfolder
#   $TR_TORRENT_HASH: Torrent SHA1 hash
# ==============================================================================

if [ -z "$TR_TORRENT_DIR" ] || [ -z "$TR_TORRENT_NAME" ]; then
    echo "[OpenUnpack-Transmission] Error: TR_TORRENT_DIR or TR_TORRENT_NAME is not set." >&2
    exit 1
fi

TARGET="$TR_TORRENT_DIR/$TR_TORRENT_NAME"
echo "[OpenUnpack-Transmission] Torrent finished: $TR_TORRENT_NAME -> $TARGET"

if command -v openunpack >/dev/null 2>&1; then
    openunpack unpack "$TARGET"
    exit $?
fi

PORT="${OPENUNPACK_PORT:-47980}"
PAYLOAD=$(cat <<EOF
{
  "command": "add_archive",
  "args": {
    "path": "$TARGET",
    "autoStart": true
  }
}
EOF
)

curl -s -X POST "http://127.0.0.1:${PORT}/api/v1/commands" \
     -H "Content-Type: application/json" \
     -d "$PAYLOAD" >/dev/null
