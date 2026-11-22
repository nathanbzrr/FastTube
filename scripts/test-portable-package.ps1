$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'portable-package.psm1') -Force
. (Join-Path $PSScriptRoot 'build-installer.ps1')

$root = Join-Path ([IO.Path]::GetTempPath()) "fasttube-portable-fixture-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $dist = Join-Path $root 'mpv'
    $bundle = Join-Path $root 'bundle'
    New-Item -ItemType Directory -Path $dist | Out-Null
    $exe = Join-Path $root 'fasttube.exe'
    $yt = Join-Path $root 'yt-dlp-explicit.exe'
    Set-Content -LiteralPath $exe -Value 'fake app'
    Set-Content -LiteralPath $yt -Value 'fake yt-dlp'
    Set-Content -LiteralPath (Join-Path $dist 'libmpv-2.dll') -Value 'fake mpv'
    Set-Content -LiteralPath (Join-Path $dist 'helper.dll') -Value 'fake helper'
    Set-Content -LiteralPath (Join-Path $dist 'user-data.json') -Value 'must not package'
    $notice = Join-Path $root 'THIRD_PARTY_NOTICES.md'
    Set-Content -LiteralPath $notice -Value 'notices fixture'

    $fakePath = Join-Path $root 'fake-path'
    New-Item -ItemType Directory -Path $fakePath | Out-Null
    $fakeYtFromPath = Join-Path $fakePath 'yt-dlp.exe'
    Set-Content -LiteralPath $fakeYtFromPath -Value 'fake PATH yt-dlp'
    $previousPath = $env:PATH
    try {
        $env:PATH = "$fakePath;$previousPath"
        if ((Resolve-PortableYtDlp) -ne $fakeYtFromPath) { throw 'yt-dlp on PATH was not resolved.' }
    } finally {
        $env:PATH = $previousPath
    }

    $dlls = @(Get-PortableMpvDlls -Directory $dist)
    if ($dlls.Count -ne 2) { throw "Expected 2 DLLs, got $($dlls.Count)." }
    New-PortableBundle -Executable $exe -YtDlp (Resolve-PortableYtDlp -Path $yt) -MpvDlls $dlls -AdditionalFiles @($notice) -Destination $bundle
    $expected = @('fasttube.exe', 'yt-dlp.exe', 'libmpv-2.dll', 'helper.dll', 'THIRD_PARTY_NOTICES.md') | Sort-Object
    $actual = @(Get-ChildItem -LiteralPath $bundle -File | Select-Object -ExpandProperty Name | Sort-Object)
    if (Compare-Object $expected $actual) { throw "Incorrect bundle contents: $($actual -join ', ')" }
    if (Test-Path -LiteralPath (Join-Path $bundle 'user-data.json')) { throw 'User file was unexpectedly packaged.' }
    if ((Get-Content -Raw -LiteralPath (Join-Path $bundle 'yt-dlp.exe')).Trim() -ne 'fake yt-dlp') { throw 'Explicit yt-dlp was not copied.' }

    $missing = Join-Path $root 'missing'
    New-Item -ItemType Directory -Path $missing | Out-Null
    try { Get-PortableMpvDlls -Directory $missing | Out-Null; throw 'Distribution without libmpv was accepted.' }
    catch { if ($_.Exception.Message -eq 'Distribution without libmpv was accepted.') { throw } }
    try { Resolve-PortableYtDlp -Path (Join-Path $root 'absent.exe') | Out-Null; throw 'Missing yt-dlp was accepted.' }
    catch { if ($_.Exception.Message -eq 'Missing yt-dlp was accepted.') { throw } }
    try { New-PortableBundle -Executable $exe -YtDlp $yt -MpvDlls $dlls -Destination $bundle; throw 'Non-empty destination was overwritten.' }
    catch { if ($_.Exception.Message -eq 'Non-empty destination was overwritten.') { throw } }

    # --- Offline SHA2-256SUMS parser and SHA-256 verifier tests ---
    $ytHash = (Get-FileHash -LiteralPath $yt -Algorithm SHA256).Hash.ToLowerInvariant()
    $mpvHash = (Get-FileHash -LiteralPath (Join-Path $dist 'libmpv-2.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
    $helperHash = (Get-FileHash -LiteralPath (Join-Path $dist 'helper.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
    $wrongHash = ('0' * 64)

    $sumsManifest = @(
        '# Sample yt-dlp SHA2-256SUMS manifest'
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  yt-dlp.exe.sig"
        "$($ytHash.ToUpperInvariant())  yt-dlp.exe"
        "$mpvHash *./libmpv-2.dll"
    ) -join "`n"

    $parsedYtHash = Get-ExpectedSha256FromSums -ChecksumContent $sumsManifest -FileName 'yt-dlp.exe'
    if ($parsedYtHash -ne $ytHash) {
        throw "Get-ExpectedSha256FromSums returned '$parsedYtHash', expected '$ytHash'."
    }
    $parsedMpvHash = Get-ExpectedSha256FromSums -ChecksumContent $sumsManifest -FileName 'libmpv-2.dll'
    if ($parsedMpvHash -ne $mpvHash) {
        throw "Get-ExpectedSha256FromSums failed on asterisk/dot-slash entry: '$parsedMpvHash'."
    }

    try {
        Get-ExpectedSha256FromSums -ChecksumContent $sumsManifest -FileName 'absent-file.exe' | Out-Null
        throw 'Missing entry in SHA2-256SUMS was accepted.'
    } catch {
        if ($_.Exception.Message -eq 'Missing entry in SHA2-256SUMS was accepted.') { throw }
    }

    $verifiedYt = Assert-FileSha256 -FilePath $yt -ExpectedSha256 ("sha256:" + $ytHash.ToUpperInvariant()) -Label 'yt-dlp.exe'
    if ($verifiedYt -ne $ytHash) {
        throw "Assert-FileSha256 returned unexpected hash: $verifiedYt"
    }

    try {
        Assert-FileSha256 -FilePath $yt -ExpectedSha256 $wrongHash -Label 'yt-dlp.exe' | Out-Null
        throw 'SHA-256 mismatch was accepted.'
    } catch {
        if ($_.Exception.Message -eq 'SHA-256 mismatch was accepted.') { throw }
    }

    try {
        Assert-FileSha256 -FilePath $yt -ExpectedSha256 'not-a-sha256' -Label 'yt-dlp.exe' | Out-Null
        throw 'Malformed SHA-256 string was accepted.'
    } catch {
        if ($_.Exception.Message -eq 'Malformed SHA-256 string was accepted.') { throw }
    }

    # Verify ExpectedMpvSha256 in both single-string mode and hashtable map mode.
    Assert-MpvDllChecksums -MpvDlls $dlls -ExpectedMpvSha256 $mpvHash
    Assert-MpvDllChecksums -MpvDlls $dlls -ExpectedMpvSha256 @{
        'libmpv-2.dll' = $mpvHash
        'helper.dll'   = $helperHash
    }
    try {
        Assert-MpvDllChecksums -MpvDlls $dlls -ExpectedMpvSha256 $wrongHash
        throw 'Mismatched libmpv SHA-256 was accepted.'
    } catch {
        if ($_.Exception.Message -eq 'Mismatched libmpv SHA-256 was accepted.') { throw }
    }
    try {
        Assert-MpvDllChecksums -MpvDlls $dlls -ExpectedMpvSha256 @{ 'missing.dll' = $mpvHash }
        throw 'Missing pinned libmpv DLL map entry was accepted.'
    } catch {
        if ($_.Exception.Message -eq 'Missing pinned libmpv DLL map entry was accepted.') { throw }
    }

    # --- Offline vswhere.exe and fallback vcvars64.bat discovery tests ---
    $fakeVsWhere = Join-Path $root 'vswhere.exe'
    Set-Content -LiteralPath $fakeVsWhere -Value 'stub'

    $vsInstallDir = Join-Path $root 'VS2022\BuildTools'
    $vsVcVarsDir = Join-Path $vsInstallDir 'VC\Auxiliary\Build'
    New-Item -ItemType Directory -Path $vsVcVarsDir -Force | Out-Null
    $vsVcVarsFile = Join-Path $vsVcVarsDir 'vcvars64.bat'
    Set-Content -LiteralPath $vsVcVarsFile -Value '@echo off'

    $fallbackVcVarsDir = Join-Path $root 'FallbackVS\VC\Auxiliary\Build'
    New-Item -ItemType Directory -Path $fallbackVcVarsDir -Force | Out-Null
    $fallbackVcVarsFile = Join-Path $fallbackVcVarsDir 'vcvars64.bat'
    Set-Content -LiteralPath $fallbackVcVarsFile -Value '@echo off'

    # 1. vswhere discovery succeeds and takes precedence over fallback candidates.
    $discoveredViaVsWhere = Find-VcVars64Path `
        -VsWhereCandidates @($fakeVsWhere) `
        -FallbackCandidates @($fallbackVcVarsFile) `
        -InvokeVsWhere { param($exePath, $exeArgs) return $vsInstallDir }
    if ($discoveredViaVsWhere -ne $vsVcVarsFile) {
        throw "Find-VcVars64Path did not prefer vswhere result. Got: '$discoveredViaVsWhere'"
    }

    # 2. When vswhere returns no valid install path, fallback candidates are used cleanly.
    $discoveredViaFallback = Find-VcVars64Path `
        -VsWhereCandidates @($fakeVsWhere) `
        -FallbackCandidates @((Join-Path $root 'nonexistent\vcvars64.bat'), $fallbackVcVarsFile) `
        -InvokeVsWhere { param($exePath, $exeArgs) return '' }
    if ($discoveredViaFallback -ne $fallbackVcVarsFile) {
        throw "Find-VcVars64Path did not fall back to candidate path. Got: '$discoveredViaFallback'"
    }

    # 3. When neither vswhere nor fallbacks exist, returns $null cleanly.
    $discoveredNone = Find-VcVars64Path `
        -VsWhereCandidates @((Join-Path $root 'missing-vswhere.exe')) `
        -FallbackCandidates @((Join-Path $root 'missing-vcvars64.bat'))
    if ($null -ne $discoveredNone) {
        throw "Find-VcVars64Path should return null when nothing exists. Got: '$discoveredNone'"
    }

    Write-Host 'Portable packaging, SHA-256 verification, and vswhere discovery checks passed.'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
