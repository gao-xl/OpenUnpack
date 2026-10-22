#!/usr/bin/env bash
set -e

REPO_ROOT="C:/Users/im/Documents/ChatGPT/批量解压缩"
MSVC_ROOT="C:/Program Files (x86)/Microsoft Visual Studio/18/BuildTools/VC/Tools/MSVC/14.50.35717"
SDK_LIB="$REPO_ROOT/artifacts/toolchain/sdk/microsoft.windows.sdk.cpp.x64/c"
SDK_INC="$REPO_ROOT/artifacts/toolchain/sdk/microsoft.windows.sdk.cpp/c/Include/10.0.26100.0"

export PATH="/c/Program Files (x86)/Microsoft Visual Studio/18/BuildTools/VC/Tools/MSVC/14.50.35717/bin/Hostx64/x64:/c/Users/im/Documents/ChatGPT/批量解压缩/artifacts/toolchain/sdk/microsoft.windows.sdk.cpp/c/bin/10.0.26100.0/x64:$PATH"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="$MSVC_ROOT/bin/Hostx64/x64/link.exe"
export CC="$MSVC_ROOT/bin/Hostx64/x64/cl.exe"
export CXX="$MSVC_ROOT/bin/Hostx64/x64/cl.exe"
export AR="$MSVC_ROOT/bin/Hostx64/x64/lib.exe"
export LIB="$MSVC_ROOT/lib/x64;$SDK_LIB/um/x64;$SDK_LIB/ucrt/x64"
export INCLUDE="$MSVC_ROOT/include;$SDK_INC/ucrt;$SDK_INC/um;$SDK_INC/shared"
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL="*"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target_dev}"
export CARGO_INCREMENTAL=0

cargo "$@"
