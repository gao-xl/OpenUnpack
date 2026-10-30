#!/usr/bin/env bash
# Transmission provides TR_TORRENT_DIR and TR_TORRENT_NAME.
# Requires a configured standalone CLI and paths valid for its Windows service.
set -euo pipefail

if [[ -z "${TR_TORRENT_DIR:-}" || -z "${TR_TORRENT_NAME:-}" ]]; then
    echo '[OpenUnpack-Transmission] Torrent directory and name are required.' >&2
    exit 1
fi
target_path="$TR_TORRENT_DIR/$TR_TORRENT_NAME"
if [[ ! -e "$target_path" ]]; then
    echo '[OpenUnpack-Transmission] A completed local file or directory is required.' >&2
    exit 1
fi

cli="${OPENUNPACK_CLI:-openunpack}"
if ! command -v -- "$cli" >/dev/null 2>&1; then
    echo '[OpenUnpack-Transmission] Configure the standalone CLI through OPENUNPACK_CLI or PATH.' >&2
    exit 127
fi
exec "$cli" unpack "$target_path"
