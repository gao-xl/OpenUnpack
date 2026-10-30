#!/usr/bin/env bash
# aria2 arguments: GID, file count, first completed file path.
# Requires a configured standalone CLI and paths valid for its Windows service.
set -euo pipefail

file_count="${2:-}"
file_path="${3:-}"
if [[ ! "$file_count" =~ ^[1-9][0-9]*$ || -z "$file_path" || ! -e "$file_path" ]]; then
    echo '[OpenUnpack-aria2] A positive file count and completed local file are required.' >&2
    exit 1
fi

target_path="$file_path"
if [[ "$file_count" != 1 ]]; then
    target_path="$(dirname -- "$file_path")"
fi
cli="${OPENUNPACK_CLI:-openunpack}"
if ! command -v -- "$cli" >/dev/null 2>&1; then
    echo '[OpenUnpack-aria2] Configure the standalone CLI through OPENUNPACK_CLI or PATH.' >&2
    exit 127
fi
exec "$cli" unpack "$target_path"
