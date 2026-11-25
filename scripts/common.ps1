$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

function Get-OpenUnpackRoot {
    [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
}

function Assert-OpenUnpackWindows {
    $onWindows = if ($PSVersionTable.PSEdition -eq 'Core') { [OperatingSystem]::IsWindows() } else { [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT }
    if (-not $onWindows) { throw '此脚本需要 Windows 运行环境。' }
    if (-not [Environment]::Is64BitProcess) { throw '请使用 64 位 PowerShell。' }
    if (-not $env:PATHEXT -or $env:PATHEXT -notlike '*.EXE*') {
        $env:PATHEXT = '.COM;.EXE;.BAT;.CMD;.VBS;.VBE;.JS;.JSE;.WSF;.WSH;.MSC'
    }
    $taskUserProfile = if ($env:USERPROFILE) { $env:USERPROFILE } else { 'C:\Users\im' }
    $taskCargoBin = Join-Path $taskUserProfile '.cargo/bin'
    if ((Test-Path -LiteralPath $taskCargoBin) -and ($env:PATH -notlike "*$taskCargoBin*")) {
        $env:PATH = "$taskCargoBin;$env:PATH"
    }
    # 检测工作区路径是否包含非 ASCII 字符；MSVC cc-rs 调用 cl.exe/lib.exe 在中文路径下会静默失败 (exit 2)
    # 若包含非 ASCII 字符且未显式指定 CARGO_TARGET_DIR，则自动重定向至纯 ASCII 缓存目录
    if (-not $env:CARGO_TARGET_DIR) {
        $workspaceRoot = Get-OpenUnpackRoot
        if ($workspaceRoot -match '[^\x00-\x7F]') {
            $fallbackTarget = if ($env:LOCALAPPDATA -and ($env:LOCALAPPDATA -notmatch '[^\x00-\x7F]')) {
                Join-Path $env:LOCALAPPDATA 'openunpack-target'
            } else {
                Join-Path $taskUserProfile '.cache/openunpack-target'
            }
            if (-not (Test-Path -LiteralPath $fallbackTarget)) {
                New-Item -ItemType Directory -Path $fallbackTarget -Force | Out-Null
            }
            $env:CARGO_TARGET_DIR = $fallbackTarget
            Write-Host "[提示] 检测到工作区路径含非 ASCII 字符，自动配置 `$env:CARGO_TARGET_DIR = '$fallbackTarget' 规避 MSVC 编译缺陷。" -ForegroundColor Yellow
        }
    }
}

function Import-OpenUnpackMsvc {
    Assert-OpenUnpackWindows
    # VS2019+ environment discovery works in plain PowerShell as well as CI.
    # Only public build variables are imported; do not print or export all env.
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (Test-OpenUnpackMsvcReady) {
        return
    }
    $taskSystemRoot = if ($env:SystemRoot) { $env:SystemRoot } else { 'C:\Windows' }
    $taskPfX86 = if (${env:ProgramFiles(x86)}) { ${env:ProgramFiles(x86)} } else { 'C:\Program Files (x86)' }
    $taskPf = if ($env:ProgramFiles) { $env:ProgramFiles } else { 'C:\Program Files' }
    $taskCandidates = [Collections.Generic.List[string]]::new()
    $taskVswhere = Join-Path $taskPfX86 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $taskVswhere) {
        $taskInstallations = @(& $taskVswhere -all -products '*' -property installationPath)
        foreach ($taskInstallation in $taskInstallations) {
            if (-not [string]::IsNullOrWhiteSpace($taskInstallation)) {
                $taskCandidates.Add((Join-Path $taskInstallation 'VC/Auxiliary/Build/vcvars64.bat'))
            }
        }
    }
    $taskCandidates.Add((Join-Path $taskPfX86 'Microsoft Visual Studio/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat'))
    $taskCandidates.Add((Join-Path $taskPf 'Microsoft Visual Studio/2022/BuildTools/VC/Auxiliary/Build/vcvars64.bat'))
    $taskVcvars = $taskCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
    if (-not $taskVcvars) { throw '缺少 Visual Studio C++ x64 Build Tools。请安装 C++ 桌面开发与 Windows SDK，或从 x64 Native Tools PowerShell 启动。' }
    # The batch command only reads environment variables. All filesystem writes
    # and removals elsewhere in these scripts stay in native PowerShell.
    $taskAllowed = @('PATH', 'INCLUDE', 'LIB', 'LIBPATH', 'VCToolsInstallDir', 'VCINSTALLDIR', 'WindowsSdkDir', 'WindowsSDKVersion', 'WindowsSDKLibVersion', 'UniversalCRTSdkDir', 'UCRTVersion')
    $taskReadVars = ($taskAllowed | ForEach-Object { 'set ' + $_ }) -join ' & '
    # `&&` would skip every `set` when vcvars64.bat returns a nonzero code (a
    # missing optional variable or a warning is enough). Use `&` so the build
    # variables are always collected, then validate the real toolchain below.
    $taskBatchCommand = 'call "' + $taskVcvars + '" >nul & (' + $taskReadVars + ')'
    $taskEnvironmentLines = @(& (Join-Path $taskSystemRoot 'System32/cmd.exe') /d /s /c $taskBatchCommand 2>&1)
    # cmd's `set NAME` returns nonzero for an optional absent variable. Validate
    # the actual compiler/linker/SDK below instead of mistaking that for vcvars failure.
    foreach ($taskLine in $taskEnvironmentLines) {
        $taskMatch = [regex]::Match([string]$taskLine, '^([^=]+)=(.*)$')
        if ($taskMatch.Success -and $taskMatch.Groups[1].Value -in $taskAllowed) {
            [Environment]::SetEnvironmentVariable($taskMatch.Groups[1].Value, $taskMatch.Groups[2].Value, 'Process')
        }
    }
    $taskUserProfile = if ($env:USERPROFILE) { $env:USERPROFILE } else { 'C:\Users\im' }
    $taskCargoBin = Join-Path $taskUserProfile '.cargo/bin'
    if ((Test-Path -LiteralPath $taskCargoBin) -and ($env:PATH -notlike "*$taskCargoBin*")) {
        $env:PATH = "$taskCargoBin;$env:PATH"
    }
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not (Test-OpenUnpackMsvcReady)) {
        Import-OpenUnpackProjectSdk -VcvarsPath $taskVcvars
    }
    if (-not (Test-OpenUnpackMsvcReady)) {
        throw ('缺少 Microsoft x64 cl/link 或 Windows SDK 实际头文件、库和资源工具。请通过 Visual Studio Installer 安装 C++ 桌面开发及 Windows 10/11 SDK；也可使用项目内已校验的官方 SDK。仅存在SDK目录或环境变量不算就绪。诊断：' +
            (Get-OpenUnpackMsvcDiagnostics))
    }
}

function Import-OpenUnpackProjectSdk {
    param([string]$VcvarsPath)
    # Fallback uses already downloaded official packages only; no network,
    # installation, registry changes, system-folder writes or SDK copying.
    $taskSdkRoot = Join-Path (Get-OpenUnpackRoot) 'artifacts/toolchain/sdk'
    $taskCpp = Join-Path $taskSdkRoot 'microsoft.windows.sdk.cpp/c'
    $taskCppX64 = Join-Path $taskSdkRoot 'microsoft.windows.sdk.cpp.x64/c'
    $taskIncludeRoot = Join-Path $taskCpp 'Include'
    if (-not (Test-Path -LiteralPath $taskIncludeRoot -PathType Container)) { return }
    $taskSdkVersion = Get-ChildItem -LiteralPath $taskIncludeRoot -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if (-not $taskSdkVersion) { return }
    $taskVersion = $taskSdkVersion.Name
    $taskSdkBin = Join-Path $taskCpp "bin/$taskVersion/x64"
    $taskVcRoot = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetDirectoryName($VcvarsPath)) '../..'))
    $taskMsvcRoot = Join-Path $taskVcRoot 'Tools/MSVC'
    if (-not (Test-Path -LiteralPath $taskMsvcRoot -PathType Container)) { return }
    $taskMsvcVersion = Get-ChildItem -LiteralPath $taskMsvcRoot -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if (-not $taskMsvcVersion) { return }
    $taskCompilerBin = Join-Path $taskMsvcVersion.FullName 'bin/Hostx64/x64'
    $taskIncludes = @((Join-Path $taskMsvcVersion.FullName 'include')) + @('ucrt','um','shared','winrt','cppwinrt' | ForEach-Object { Join-Path $taskSdkVersion.FullName $_ })
    $taskLibraries = @((Join-Path $taskMsvcVersion.FullName 'lib/x64'), (Join-Path $taskCppX64 'ucrt/x64'), (Join-Path $taskCppX64 'um/x64'))
    $taskRequired = @((Join-Path $taskCompilerBin 'cl.exe'), (Join-Path $taskCompilerBin 'link.exe'),
        (Join-Path $taskSdkVersion.FullName 'um/Windows.h'), (Join-Path $taskSdkVersion.FullName 'ucrt/stdio.h'),
        (Join-Path $taskCppX64 'um/x64/kernel32.lib'), (Join-Path $taskCppX64 'ucrt/x64/ucrt.lib'),
        (Join-Path $taskSdkBin 'rc.exe'), (Join-Path $taskSdkBin 'rcdll.dll'), (Join-Path $taskSdkBin 'mt.exe'))
    if ($taskRequired | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) }) { return }
    if (@($taskIncludes + $taskLibraries) | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Container) }) { return }
    $env:PATH = (@($taskCompilerBin, $taskSdkBin) + @($env:PATH.Split(';') | Where-Object { $_ -and $_ -notin @($taskCompilerBin, $taskSdkBin) })) -join ';'
    $env:INCLUDE = $taskIncludes -join ';'
    $env:LIB = $taskLibraries -join ';'
    $env:LIBPATH = (Join-Path $taskMsvcVersion.FullName 'lib/x64')
    $env:VCINSTALLDIR = $taskVcRoot.TrimEnd('\') + '\'
    $env:VCToolsInstallDir = $taskMsvcVersion.FullName.TrimEnd('\') + '\'
    $env:VCToolsVersion = $taskMsvcVersion.Name
    $env:WindowsSdkDir = $taskCpp.TrimEnd('\') + '\'
    $env:WindowsSDKVersion = $taskVersion + '\'
    $env:WindowsSDKLibVersion = $taskVersion + '\'
    $env:UniversalCRTSdkDir = $taskCpp.TrimEnd('\') + '\'
    $env:UCRTVersion = $taskVersion
    $env:VSCMD_ARG_TGT_ARCH = 'x64'
}

function Get-OpenUnpackMsvcDiagnostics {
    $taskParts = [Collections.Generic.List[string]]::new()
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $taskParts.Add('cl=' + $(if ($taskCompiler) { $taskCompiler.Source } else { '<none>' }))
    $taskParts.Add('link=' + $(if ($taskLinker) { $taskLinker.Source } else { '<none>' }))
    $taskParts.Add('INCLUDE=' + $(if ($env:INCLUDE) { $env:INCLUDE } else { '<empty>' }))
    $taskParts.Add('LIB=' + $(if ($env:LIB) { $env:LIB } else { '<empty>' }))
    foreach ($taskHeader in @('Windows.h', 'stdio.h')) {
        $taskFound = $false
        foreach ($taskDirectory in @($env:INCLUDE.Split(';') | Where-Object { $_ })) {
            if (Test-Path -LiteralPath (Join-Path $taskDirectory $taskHeader) -PathType Leaf) { $taskFound = $true; break }
        }
        $taskParts.Add($taskHeader + '=' + $taskFound)
    }
    foreach ($taskLibrary in @('kernel32.lib', 'ucrt.lib')) {
        $taskFound = $false
        foreach ($taskDirectory in @($env:LIB.Split(';') | Where-Object { $_ })) {
            if (Test-Path -LiteralPath (Join-Path $taskDirectory $taskLibrary) -PathType Leaf) { $taskFound = $true; break }
        }
        $taskParts.Add($taskLibrary + '=' + $taskFound)
    }
    foreach ($taskSdkTool in @('rc.exe', 'mt.exe')) {
        $taskTool = Get-Command $taskSdkTool -CommandType Application -ErrorAction SilentlyContinue
        $taskParts.Add($taskSdkTool + '=' + $(if ($taskTool) { $taskTool.Source } else { '<none>' }))
    }
    return ($taskParts -join ' | ')
}

function Test-OpenUnpackMsvcReady {
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $taskCompiler -or -not $taskLinker -or -not $env:INCLUDE -or -not $env:LIB -or
        $taskCompiler.Source -notmatch '\\VC\\Tools\\MSVC\\.*\\[xX]64\\cl\.exe$' -or
        $taskLinker.Source -notmatch '\\VC\\Tools\\MSVC\\.*\\[xX]64\\link\.exe$') { return $false }
    $taskIncludes = @($env:INCLUDE.Split(';') | Where-Object { $_ })
    $taskLibraries = @($env:LIB.Split(';') | Where-Object { $_ })
    foreach ($taskHeader in @('Windows.h', 'stdio.h')) {
        if (-not ($taskIncludes | Where-Object { Test-Path -LiteralPath (Join-Path $_ $taskHeader) -PathType Leaf } | Select-Object -First 1)) { return $false }
    }
    foreach ($taskLibrary in @('kernel32.lib', 'ucrt.lib')) {
        if (-not ($taskLibraries | Where-Object { Test-Path -LiteralPath (Join-Path $_ $taskLibrary) -PathType Leaf } | Select-Object -First 1)) { return $false }
    }
    foreach ($taskSdkTool in @('rc.exe', 'mt.exe')) {
        if (-not (Get-Command $taskSdkTool -CommandType Application -ErrorAction SilentlyContinue)) { return $false }
    }
    return $true
}

function Invoke-OpenUnpackTool {
    param([string]$Name, [Parameter(ValueFromRemainingArguments = $true)]$Arguments)
    $argsList = @($Arguments | ForEach-Object { $_ })
    & $Name @argsList
    if ($LASTEXITCODE -ne 0) { throw "$Name 执行失败，退出码：$LASTEXITCODE" }
}

function New-OpenUnpackDirectory {
    param([string]$Path)
    New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Assert-OpenUnpackOwnedPath {
    param([string]$Path, [string]$Parent)
    $resolved = [IO.Path]::GetFullPath($Path)
    $boundary = [IO.Path]::GetFullPath($Parent).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith($boundary, [StringComparison]::OrdinalIgnoreCase)) {
        throw '构建输出路径越过允许的目录。'
    }
    $ancestor = [IO.Path]::GetDirectoryName($resolved)
    while ($ancestor) {
        if ((Test-Path -LiteralPath $ancestor) -and ((Get-Item -LiteralPath $ancestor).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw '构建输出父路径包含链接，拒绝清理。'
        }
        $ancestor = [IO.Path]::GetDirectoryName($ancestor)
    }
    if (Test-Path -LiteralPath $resolved) {
        foreach ($item in @(Get-Item -LiteralPath $resolved) + @(Get-ChildItem -LiteralPath $resolved -Recurse -Force)) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw '构建输出目录包含链接，拒绝清理。' }
        }
    }
    $resolved
}

function Reset-OpenUnpackBuildDirectory {
    param([string]$Path, [string]$Parent)
    $resolved = Assert-OpenUnpackOwnedPath -Path $Path -Parent $Parent
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
    New-OpenUnpackDirectory $resolved
}

function Write-OpenUnpackUtf8NoBom {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][AllowEmptyString()][string]$Content)
    [IO.File]::WriteAllText($Path, $Content, [Text.UTF8Encoding]::new($false))
}

function Write-OpenUnpackChecksums {
    param([string]$Directory, [string]$Output = (Join-Path $Directory 'SHA256SUMS.txt'))
    $base = [IO.Path]::GetFullPath($Directory).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    $lines = foreach ($file in Get-ChildItem -LiteralPath $Directory -File -Recurse | Sort-Object FullName) {
        if ($file.FullName -eq [IO.Path]::GetFullPath($Output)) { continue }
        $full = [IO.Path]::GetFullPath($file.FullName)
        $relative = if ($full.StartsWith($base, [StringComparison]::OrdinalIgnoreCase)) {
            $full.Substring($base.Length).Replace('\', '/')
        } else {
            [IO.Path]::GetFileName($full)
        }
        '{0}  {1}' -f (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
    }
    [IO.File]::WriteAllText($Output, ($lines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
}
