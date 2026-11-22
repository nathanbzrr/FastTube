[CmdletBinding()]
param(
    [string] $LibMpvDirectory,
    [string] $YtDlpPath,
    [string] $ExecutablePath,
    [string] $TargetDirectory,
    [string] $StagingDirectory,
    [string] $OutputDirectory,
    [string] $AppVersion,
    [string] $InnoSetupCompiler,
    [switch] $DownloadStandaloneYtDlp,
    [string] $YtDlpReleaseBaseUrl = 'https://github.com/yt-dlp/yt-dlp/releases/latest/download',
    [object] $ExpectedMpvSha256
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-ExpectedSha256FromSums {
    <#
    .SYNOPSIS
        Parses a SHA2-256SUMS / SHA256SUMS file and returns the normalized lowercase SHA-256 hash for FileName.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string] $ChecksumContent,
        [Parameter(Mandatory)][string] $FileName
    )

    $normalizedTarget = $FileName.Trim().TrimStart('*').TrimStart('./').TrimStart('.\')
    foreach ($rawLine in ($ChecksumContent -split "`r?`n")) {
        $line = $rawLine.Trim()
        if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith('#')) {
            continue
        }
        if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
            $candidateHash = $Matches[1].ToLowerInvariant()
            $candidateName = $Matches[2].Trim()
            if ($candidateName.StartsWith('./') -or $candidateName.StartsWith('.\')) {
                $candidateName = $candidateName.Substring(2)
            }
            if ($candidateName -ieq $normalizedTarget) {
                return $candidateHash
            }
        }
    }

    throw "SHA-256 entry for '$FileName' was not found in checksum manifest."
}

function Assert-FileSha256 {
    <#
    .SYNOPSIS
        Verifies a file's SHA-256 digest against an expected 64-character hex digest (optional 'sha256:' prefix).
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string] $FilePath,
        [Parameter(Mandatory)][string] $ExpectedSha256,
        [string] $Label = ''
    )

    if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
        throw "Cannot verify SHA-256 because file does not exist: $FilePath"
    }

    $normalizedExpected = $ExpectedSha256.Trim()
    if ($normalizedExpected -match '^sha256:(.+)$') {
        $normalizedExpected = $Matches[1].Trim()
    }
    if ($normalizedExpected -notmatch '^[0-9a-fA-F]{64}$') {
        throw "Invalid expected SHA-256 format for '$FilePath': '$ExpectedSha256'"
    }
    $normalizedExpected = $normalizedExpected.ToLowerInvariant()

    $actualHash = (Get-FileHash -LiteralPath $FilePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $normalizedExpected) {
        $displayName = if (-not [string]::IsNullOrWhiteSpace($Label)) { $Label } else { [IO.Path]::GetFileName($FilePath) }
        throw "SHA-256 verification failed for '$displayName' ($FilePath). Expected: $normalizedExpected, Actual: $actualHash"
    }

    return $actualHash
}

function Assert-MpvDllChecksums {
    <#
    .SYNOPSIS
        Verifies staged/discovered libmpv DLLs against either a single pinned SHA-256 string (primary mpv DLL)
        or a hashtable/dictionary mapping DLL filename -> expected SHA-256.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][object[]] $MpvDlls,
        [Parameter(Mandatory = $false)][AllowNull()][object] $ExpectedMpvSha256
    )

    if ($null -eq $ExpectedMpvSha256) {
        return
    }

    if ($ExpectedMpvSha256 -is [string]) {
        if ([string]::IsNullOrWhiteSpace($ExpectedMpvSha256)) {
            return
        }
        $primaryDll = @($MpvDlls | Where-Object { $_.Name -ieq 'libmpv-2.dll' -or $_.Name -ieq 'mpv-2.dll' } | Select-Object -First 1)
        if ($primaryDll.Count -eq 0) {
            throw 'Unable to locate primary libmpv-2.dll or mpv-2.dll for SHA-256 verification.'
        }
        [void](Assert-FileSha256 -FilePath $primaryDll[0].FullName -ExpectedSha256 $ExpectedMpvSha256 -Label $primaryDll[0].Name)
        return
    }

    if ($ExpectedMpvSha256 -is [System.Collections.IDictionary]) {
        if ($ExpectedMpvSha256.Count -eq 0) {
            throw 'ExpectedMpvSha256 map must not be empty.'
        }
        foreach ($dllName in $ExpectedMpvSha256.Keys) {
            $matchedDll = @($MpvDlls | Where-Object { $_.Name -ieq $dllName } | Select-Object -First 1)
            if ($matchedDll.Count -eq 0) {
                throw "Expected libmpv DLL '$dllName' from ExpectedMpvSha256 map was not found in LibMpvDirectory."
            }
            [void](Assert-FileSha256 -FilePath $matchedDll[0].FullName -ExpectedSha256 ([string]$ExpectedMpvSha256[$dllName]) -Label $matchedDll[0].Name)
        }
        return
    }

    throw "ExpectedMpvSha256 must be a SHA-256 string or a hashtable mapping DLL filenames to SHA-256 digests."
}

function Find-VcVars64Path {
    <#
    .SYNOPSIS
        Locates MSVC vcvars64.bat dynamically via vswhere.exe, falling back to known install locations.
    #>
    [CmdletBinding()]
    param(
        [string[]] $VsWhereCandidates,
        [string[]] $FallbackCandidates,
        [scriptblock] $InvokeVsWhere
    )

    $vsWherePaths = [System.Collections.Generic.List[string]]::new()
    if ($PSBoundParameters.ContainsKey('VsWhereCandidates')) {
        foreach ($cand in @($VsWhereCandidates)) {
            if (-not [string]::IsNullOrWhiteSpace($cand)) {
                $vsWherePaths.Add($cand)
            }
        }
    } else {
        foreach ($defaultVsWhere in @(
                (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'),
                (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\Installer\vswhere.exe')
            )) {
            if (-not [string]::IsNullOrWhiteSpace($defaultVsWhere)) {
                $vsWherePaths.Add($defaultVsWhere)
            }
        }
        $vsWhereCmd = Get-Command vswhere.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($null -ne $vsWhereCmd -and -not [string]::IsNullOrWhiteSpace($vsWhereCmd.Source)) {
            $vsWherePaths.Add($vsWhereCmd.Source)
        }
    }

    $checkedVsWhere = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    foreach ($vsWherePath in $vsWherePaths) {
        if (-not $checkedVsWhere.Add($vsWherePath)) { continue }
        if (-not (Test-Path -LiteralPath $vsWherePath -PathType Leaf)) { continue }

        $vsWhereArgs = @(
            '-latest',
            '-products', '*',
            '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
            '-property', 'installationPath'
        )
        $installPaths = @()
        try {
            if ($null -ne $InvokeVsWhere) {
                $installPaths = @(& $InvokeVsWhere $vsWherePath $vsWhereArgs)
            } else {
                $installPaths = @(& $vsWherePath @vsWhereArgs 2>$null)
            }
        } catch {
            $installPaths = @()
        }

        foreach ($installPath in $installPaths) {
            if ([string]::IsNullOrWhiteSpace([string]$installPath)) { continue }
            $candidateVcVars = Join-Path ([string]$installPath).Trim() 'VC\Auxiliary\Build\vcvars64.bat'
            if (Test-Path -LiteralPath $candidateVcVars -PathType Leaf) {
                return $candidateVcVars
            }
        }
    }

    $fallbacks = if ($PSBoundParameters.ContainsKey('FallbackCandidates')) {
        @($FallbackCandidates)
    } else {
        @(
            'D:\VS\BuildTools\VC\Auxiliary\Build\vcvars64.bat',
            "${env:ProgramFiles}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
            "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
            "${env:ProgramFiles}\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat",
            "${env:ProgramFiles}\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat",
            "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
        )
    }

    foreach ($fallback in $fallbacks) {
        if (-not [string]::IsNullOrWhiteSpace($fallback) -and (Test-Path -LiteralPath $fallback -PathType Leaf)) {
            return $fallback
        }
    }

    return $null
}

# Allow offline tests and release workflows to dot-source checksum and discovery helpers.
if ($MyInvocation.InvocationName -eq '.') {
    return
}

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $LibMpvDirectory) {
    $LibMpvDirectory = $repoRoot
}
Import-Module (Join-Path $PSScriptRoot 'portable-package.psm1') -Force

if (-not $AppVersion) {
    $cargoToml = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
    if ($cargoToml -match "(?m)^version\s*=\s*['""]([^'""]+)['""]") {
        $AppVersion = $Matches[1]
    } else {
        throw 'Unable to determine package version from Cargo.toml.'
    }
}

if (-not $TargetDirectory) {
    $TargetDirectory = Join-Path $repoRoot 'target\release-build'
}
if (-not $StagingDirectory) {
    $StagingDirectory = Join-Path $repoRoot 'target\installer-staging'
}
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $repoRoot 'target\installer'
}

foreach ($pathName in @('TargetDirectory', 'StagingDirectory', 'OutputDirectory', 'ExecutablePath')) {
    $pathValue = Get-Variable -Name $pathName -ValueOnly
    if ($pathValue -and -not [IO.Path]::IsPathRooted($pathValue)) {
        Set-Variable -Name $pathName -Value ([IO.Path]::GetFullPath((Join-Path $repoRoot $pathValue)))
    }
}

if (-not $InnoSetupCompiler) {
    $cmd = Get-Command iscc -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) {
        $InnoSetupCompiler = $cmd.Source
    } else {
        $candidates = @(
            (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
            (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
            (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
        )
        $InnoSetupCompiler = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
    }
}
if (-not $InnoSetupCompiler -or -not (Test-Path -LiteralPath $InnoSetupCompiler -PathType Leaf)) {
    throw 'Inno Setup 6 (ISCC.exe) not found. Install JRSoftware.InnoSetup or pass -InnoSetupCompiler.'
}

$mpvDlls = @(Get-PortableMpvDlls -Directory $LibMpvDirectory)
Assert-MpvDllChecksums -MpvDlls $mpvDlls -ExpectedMpvSha256 $ExpectedMpvSha256

if (-not $ExecutablePath) {
    $standardTarget = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target'))
    $resolvedTarget = [IO.Path]::GetFullPath($TargetDirectory)
    if ($resolvedTarget.TrimEnd('\') -ieq $standardTarget.TrimEnd('\')) {
        throw "TargetDirectory must be isolated from Cargo's default target directory: $standardTarget"
    }

    $hasLink = [bool](Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue)
    Push-Location $repoRoot
    try {
        if ($hasLink) {
            & cargo build --release --locked --manifest-path (Join-Path $repoRoot 'Cargo.toml') --target-dir $TargetDirectory
        } else {
            $vcvars = Find-VcVars64Path
            if (-not $vcvars) {
                throw 'MSVC link.exe not found on PATH and vcvars64.bat could not be located via vswhere.exe or standard install paths.'
            }
            $manifestPath = Join-Path $repoRoot 'Cargo.toml'
            & cmd.exe /c "call `"$vcvars`" && cargo build --release --locked --manifest-path `"$manifestPath`" --target-dir `"$TargetDirectory`""
        }
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
    $ExecutablePath = Join-Path $TargetDirectory 'release\fasttube.exe'
}

if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "Release executable not found: $ExecutablePath"
}

New-Item -ItemType Directory -Path $StagingDirectory -Force | Out-Null
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

$stagedYtDlp = Join-Path $StagingDirectory 'yt-dlp.exe'
$preservedYtDlp = $false
if ($YtDlpPath) {
    $resolvedYtDlp = Resolve-PortableYtDlp -Path $YtDlpPath
    if ([IO.Path]::GetFullPath($resolvedYtDlp) -ieq [IO.Path]::GetFullPath($stagedYtDlp)) {
        $preservedYtDlp = $true
    }
}

Get-ChildItem -LiteralPath $StagingDirectory -Force | Where-Object {
    -not ($preservedYtDlp -and ($_.FullName -ieq $stagedYtDlp))
} | Remove-Item -Recurse -Force

if ($DownloadStandaloneYtDlp -or -not $YtDlpPath) {
    $effectiveBaseUrl = $YtDlpReleaseBaseUrl.TrimEnd('/')
    if ($effectiveBaseUrl -ieq 'https://github.com/yt-dlp/yt-dlp/releases/latest/download') {
        try {
            # Resolve immutable tag URL so yt-dlp.exe and SHA2-256SUMS come from the exact same release.
            $ytRelease = Invoke-RestMethod -Uri 'https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest' -Headers @{ 'User-Agent' = 'FastTube-Installer-Builder' }
            if ($null -ne $ytRelease -and -not [string]::IsNullOrWhiteSpace([string]$ytRelease.tag_name)) {
                $effectiveBaseUrl = "https://github.com/yt-dlp/yt-dlp/releases/download/$($ytRelease.tag_name)"
            }
        } catch {
            # Fall back to the /releases/latest/download URL when GitHub API is rate-limited or unreachable.
            $effectiveBaseUrl = $YtDlpReleaseBaseUrl.TrimEnd('/')
        }
    }

    $ytDlpUrl = "$effectiveBaseUrl/yt-dlp.exe"
    $ytDlpSumsUrl = "$effectiveBaseUrl/SHA2-256SUMS"
    $tempSumsPath = Join-Path ([IO.Path]::GetTempPath()) ("fasttube-ytdlp-sha256sums-" + [Guid]::NewGuid().ToString('N') + '.txt')

    Write-Host "Downloading standalone yt-dlp.exe from $ytDlpUrl..."
    $prevProgress = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $ytDlpUrl -OutFile $stagedYtDlp
        Invoke-WebRequest -UseBasicParsing -Uri $ytDlpSumsUrl -OutFile $tempSumsPath

        $sumsContent = Get-Content -LiteralPath $tempSumsPath -Raw -Encoding UTF8
        $expectedYtDlpHash = Get-ExpectedSha256FromSums -ChecksumContent $sumsContent -FileName 'yt-dlp.exe'
        $verifiedHash = Assert-FileSha256 -FilePath $stagedYtDlp -ExpectedSha256 $expectedYtDlpHash -Label 'yt-dlp.exe'
        Write-Host "Verified standalone yt-dlp.exe SHA-256: $verifiedHash"
    } catch {
        if (Test-Path -LiteralPath $stagedYtDlp -PathType Leaf) {
            Remove-Item -LiteralPath $stagedYtDlp -Force -ErrorAction SilentlyContinue
        }
        throw
    } finally {
        $ProgressPreference = $prevProgress
        if (Test-Path -LiteralPath $tempSumsPath -PathType Leaf) {
            Remove-Item -LiteralPath $tempSumsPath -Force -ErrorAction SilentlyContinue
        }
    }
} elseif (-not $preservedYtDlp) {
    Copy-Item -LiteralPath $resolvedYtDlp -Destination $stagedYtDlp -Force
}

Copy-Item -LiteralPath $ExecutablePath -Destination (Join-Path $StagingDirectory 'fasttube.exe') -Force
foreach ($dll in $mpvDlls) {
    Copy-Item -LiteralPath $dll.FullName -Destination (Join-Path $StagingDirectory $dll.Name) -Force
}

$issPath = Join-Path $repoRoot 'installer\fasttube.iss'
& $InnoSetupCompiler `
    "/DAppVersion=$AppVersion" `
    "/DStagingDir=$StagingDirectory" `
    "/DOutputDir=$OutputDirectory" `
    "/DRepoRoot=$repoRoot" `
    $issPath
if ($LASTEXITCODE -ne 0) {
    throw "ISCC.exe failed with exit code $LASTEXITCODE"
}

$installerPath = Join-Path $OutputDirectory "FastTube-Setup-$AppVersion-x64.exe"
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "Expected installer was not produced: $installerPath"
}
Write-Host "Windows installer created: $installerPath"
