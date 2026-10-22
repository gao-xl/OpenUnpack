param([Parameter(Mandatory)][string]$HostExe, [Parameter(Mandatory)][string]$SetupExe,
      [Parameter(Mandatory)][string]$EvidenceDirectory, [string]$ExpectedSignerThumbprint,
      [switch]$AllowUnsigned)
. (Join-Path $PSScriptRoot 'common.ps1')
Assert-OpenUnpackWindows

function Get-OpenUnpackSignaturePolicyErrors {
    param([string]$Status, [string]$SignerThumbprint, [bool]$CodeSigningEku, [string]$ExpectedThumbprint,
          [bool]$AllowUnsigned)
    $taskErrors = [Collections.Generic.List[string]]::new()
    # 开源分发模式：未做 Authenticode 签名时，完整性由发布页的 SHA-256 清单承担，不视为门禁失败。
    # 若文件确实带签名，仍必须完整满足「有效 + CodeSigning EKU + 匹配预期发行者」三项策略。
    if ($AllowUnsigned -and $Status -ceq 'NotSigned' -and [string]::IsNullOrEmpty($SignerThumbprint)) {
        return @()
    }
    if ($Status -cne 'Valid') { $taskErrors.Add('Authenticode status is ' + $Status) }
    if (-not $CodeSigningEku) { $taskErrors.Add('CodeSigning EKU is missing') }
    if ($ExpectedThumbprint -notmatch '^[a-fA-F0-9]{40}$') {
        $taskErrors.Add('Expected signer SHA-1 certificate thumbprint is required (40 hexadecimal characters)')
    } elseif (-not [string]::Equals($SignerThumbprint, $ExpectedThumbprint, [StringComparison]::OrdinalIgnoreCase)) {
        $taskErrors.Add('Signer certificate does not match the expected publisher')
    }
    return $taskErrors.ToArray()
}

$taskRoot = Get-OpenUnpackRoot
$taskEvidence = Assert-OpenUnpackOwnedPath -Path $EvidenceDirectory -Parent (Join-Path $taskRoot 'artifacts')
if ((Test-Path -LiteralPath $taskEvidence) -and @(Get-ChildItem -LiteralPath $taskEvidence -Force).Count -ne 0) {
    throw '签名证据目录非空，拒绝覆盖历史结果。'
}
New-OpenUnpackDirectory $taskEvidence
$taskCertificateRows = [Collections.Generic.List[object]]::new()
$taskStoreErrors = [Collections.Generic.List[object]]::new()
$taskSigningCertificatePresent = $false
foreach ($taskStore in @('Cert:\CurrentUser\My', 'Cert:\LocalMachine\My')) {
    try {
        foreach ($taskCertificate in Get-ChildItem -LiteralPath $taskStore -CodeSigningCert -ErrorAction Stop) {
            # Public metadata only. Never export a certificate, private key, PFX or environment variables.
            $taskCertificateRows.Add(@{ store = $taskStore; subject = $taskCertificate.Subject;
                thumbprint = $taskCertificate.Thumbprint; hasPrivateKey = $taskCertificate.HasPrivateKey;
                notAfter = $taskCertificate.NotAfter.ToUniversalTime().ToString('o') })
            if ($ExpectedSignerThumbprint -match '^[a-fA-F0-9]{40}$' -and
                [string]::Equals($taskCertificate.Thumbprint, $ExpectedSignerThumbprint, [StringComparison]::OrdinalIgnoreCase) -and
                $taskCertificate.HasPrivateKey -and $taskCertificate.NotAfter -gt [DateTime]::Now -and $taskCertificate.NotBefore -le [DateTime]::Now) {
                $taskSigningCertificatePresent = $true
            }
        }
    } catch { $taskStoreErrors.Add(@{ store = $taskStore; error = $_.Exception.Message }) }
}
$taskFiles = [Collections.Generic.List[object]]::new()
foreach ($taskInput in @(@{ role = 'host'; path = $HostExe }, @{ role = 'setup'; path = $SetupExe })) {
    try {
        $taskFile = Get-Item -LiteralPath $taskInput.path -ErrorAction Stop
        if ($taskFile.PSIsContainer -or $taskFile.Extension -ine '.exe' -or ($taskFile.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw '签名门禁仅接受普通 EXE 文件。'
        }
        $taskHash = (Get-FileHash -LiteralPath $taskFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $taskSignature = Get-AuthenticodeSignature -LiteralPath $taskFile.FullName -ErrorAction Stop
        $taskSigner = $taskSignature.SignerCertificate
        $taskEku = $false
        if ($taskSigner) {
            foreach ($taskExtension in $taskSigner.Extensions) {
                if ($taskExtension -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]) {
                    foreach ($taskOid in $taskExtension.EnhancedKeyUsages) {
                        if ($taskOid.Value -eq '1.3.6.1.5.5.7.3.3') { $taskEku = $true }
                    }
                }
            }
        }
        $taskThumbprint = if ($taskSigner) { $taskSigner.Thumbprint } else { '' }
        $taskErrors = @(Get-OpenUnpackSignaturePolicyErrors -Status ([string]$taskSignature.Status) -SignerThumbprint $taskThumbprint -CodeSigningEku $taskEku -ExpectedThumbprint $ExpectedSignerThumbprint -AllowUnsigned $AllowUnsigned)
        if ((Get-FileHash -LiteralPath $taskFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -cne $taskHash) {
            $taskErrors += 'File changed while its signature was checked'
        }
        $taskFiles.Add(@{ role = $taskInput.role; path = $taskFile.FullName; sha256 = $taskHash;
            status = [string]$taskSignature.Status; passed = $taskErrors.Count -eq 0; errors = $taskErrors;
            integrityOnly = [bool]($AllowUnsigned -and -not $taskSigner);
            signerSubject = $(if ($taskSigner) { $taskSigner.Subject } else { $null }); signerThumbprint = $taskThumbprint;
            timestamped = $null -ne $taskSignature.TimeStamperCertificate })
    } catch {
        $taskFiles.Add(@{ role = $taskInput.role; path = $taskInput.path; passed = $false; errors = @($_.Exception.Message) })
    }
}
$taskPassed = $taskFiles.Count -eq 2 -and @($taskFiles | Where-Object { -not $_.passed }).Count -eq 0
@{ status = $(if ($taskPassed) { 'passed' } else { 'failed' }); at = [DateTimeOffset]::UtcNow.ToString('o');
    scope = 'Read-only Authenticode trust, CodeSigning EKU, pinned publisher, and SHA-256 for supplied Host and Setup; does not sign files or establish GUI/installer acceptance.';
    mode = $(if ($AllowUnsigned) { 'unsigned-oss-release' } else { 'signed-publisher' });
    unsignedAllowed = [bool]$AllowUnsigned;
    integrityControl = $(if ($AllowUnsigned) { 'SHA-256 publication manifest is the integrity control; no Authenticode signature is claimed.' } else { 'Authenticode signature with pinned publisher.' });
    expectedSignerThumbprint = $ExpectedSignerThumbprint; files = $taskFiles.ToArray();
    matchingUnexpiredPrivateKeyCertificatePresent = $taskSigningCertificatePresent;
    signingReadinessScope = 'Certificate metadata only; private-key accessibility and signing are not tested. Already signed artifacts can be verified without a local signing key.';
    codeSigningCertificates = $taskCertificateRows.ToArray(); certificateStoreErrors = $taskStoreErrors.ToArray() } |
    ConvertTo-Json -Depth 7 | Set-Content -LiteralPath (Join-Path $taskEvidence 'release-signatures.json') -Encoding utf8NoBOM
if (-not $taskPassed) {
    throw '发行签名门禁失败；详情见 release-signatures.json。未签名候选不满足发行条件（开源无签名分发请显式追加 -AllowUnsigned，改用 SHA-256 完整性模式）。'
}
if ($AllowUnsigned) {
    Write-Output 'PASS unsigned open-source release: SHA-256 recorded for Host and Setup, no Authenticode signature present (expected). Publish SHA256SUMS.txt alongside the artifacts.'
} else {
    Write-Output 'PASS Host and Setup signatures match the trusted expected publisher.'
}
