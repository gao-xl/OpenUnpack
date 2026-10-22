#!/usr/bin/env bash
# ==============================================================================
# OpenUnpack - aria2 Download Completion Hook
#
# Setup in aria2.conf:
#   on-download-complete=/path/to/aria2-on-complete.sh
#
# Arguments passed by aria2:
#   $1: GID
#   $2: File count
#   $3: File path of the first downloaded file
# ==============================================================================

GID="$1"
FILE_COUNT="$2"
FILE_PATH="$3"

if [ -z "$FILE_PATH" ] || [ ! -e "$FILE_PATH" ]; then
    echo "[OpenUnpack-aria2] No valid file path provided for GID $GID" >&2
    exit 0
fi

echo "[OpenUnpack-aria2] Download completed for GID $GID (File: $FILE_PATH)"

# If directory or multifile download, extract parent folder or file
if [ "$FILE_COUNT" -gt 1 ]; then
    TARGET_DIR="$(dirname "$FILE_PATH")"
    TARGET="$TARGET_DIR"
else
    TARGET="$FILE_PATH"
fi

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
