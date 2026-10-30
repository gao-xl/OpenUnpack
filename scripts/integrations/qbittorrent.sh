#!/usr/bin/env bash
# qBittorrent completion arguments: "%F" "%D" "%N" "%L".
# Requires a configured standalone CLI and paths valid for its Windows service.
set -euo pipefail

target_path="${1:-}"
if [[ -z "$target_path" || ! -e "$target_path" ]]; then
    echo '[OpenUnpack-Hook] A completed local file or directory is required.' >&2
    exit 1
fi

cli="${OPENUNPACK_CLI:-openunpack}"
if ! command -v -- "$cli" >/dev/null 2>&1; then
    echo '[OpenUnpack-Hook] Configure the standalone CLI through OPENUNPACK_CLI or PATH.' >&2
    exit 127
fi
exec "$cli" unpack "$target_path"
