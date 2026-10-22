$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-OpenUnpackRoot {
    [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
}

function Assert-OpenUnpackWindows {
    if (-not [OperatingSystem]::IsWindows()) { throw '此脚本需要 Windows 和 PowerShell 7。' }
    if (-not [Environment]::Is64BitProcess) { throw '请使用 64 位 PowerShell 7。' }
}

function Import-OpenUnpackMsvc {
    Assert-OpenUnpackWindows
    # VS2019+ environment discovery works in plain PowerShell as well as CI.
    # Only public build variables are imported; do not print or export all env.
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue
    if (Test-OpenUnpackMsvcReady) {
        return
    }
    $taskCandidates = [Collections.Generic.List[string]]::new()
    $taskVswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $taskVswhere) {
        $taskInstallations = @(& $taskVswhere -all -products '*' -property installationPath)
        foreach ($taskInstallation in $taskInstallations) {
            if (-not [string]::IsNullOrWhiteSpace($taskInstallation)) {
                $taskCandidates.Add((Join-Path $taskInstallation 'VC/Auxiliary/Build/vcvars64.bat'))
            }
        }
    }
    $taskCandidates.Add((Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/18/BuildTools/VC/Auxiliary/Build/vcvars64.bat'))
    $taskCandidates.Add((Join-Path ${env:ProgramFiles} 'Microsoft Visual Studio/2022/BuildTools/VC/Auxiliary/Build/vcvars64.bat'))
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
    $taskEnvironmentLines = @(& (Join-Path $env:SystemRoot 'System32/cmd.exe') /d /s /c $taskBatchCommand 2>&1)
    # cmd's `set NAME` returns nonzero for an optional absent variable. Validate
    # the actual compiler/linker/SDK below instead of mistaking that for vcvars failure.
    foreach ($taskLine in $taskEnvironmentLines) {
        $taskMatch = [regex]::Match([string]$taskLine, '^([^=]+)=(.*)$')
        if ($taskMatch.Success -and $taskMatch.Groups[1].Value -in $taskAllowed) {
            [Environment]::SetEnvironmentVariable($taskMatch.Groups[1].Value, $taskMatch.Groups[2].Value, 'Process')
        }
    }
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue
    if (-not (Test-OpenUnpackMsvcReady)) {
        Import-OpenUnpackProjectSdk -VcvarsPath $taskVcvars
    }
    if (-not (Test-OpenUnpackMsvcReady)) {
        throw '缺少 Microsoft x64 cl/link 或 Windows SDK 实际头文件、库和资源工具。请通过 Visual Studio Installer 安装 C++ 桌面开发及 Windows 10/11 SDK；也可使用项目内已校验的官方 SDK。仅存在SDK目录或环境变量不算就绪。'
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

function Test-OpenUnpackMsvcReady {
    $taskCompiler = Get-Command cl.exe -CommandType Application -ErrorAction SilentlyContinue
    $taskLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue
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
    param([string]$Name, [Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & $Name @Arguments
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

function Write-OpenUnpackChecksums {
    param([string]$Directory, [string]$Output = (Join-Path $Directory 'SHA256SUMS.txt'))
    $lines = foreach ($file in Get-ChildItem -LiteralPath $Directory -File -Recurse | Sort-Object FullName) {
        if ($file.FullName -eq [IO.Path]::GetFullPath($Output)) { continue }
        $relative = [IO.Path]::GetRelativePath($Directory, $file.FullName).Replace('\', '/')
        '{0}  {1}' -f (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
    }
    [IO.File]::WriteAllText($Output, ($lines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
}
