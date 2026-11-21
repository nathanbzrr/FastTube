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
    [switch] $DownloadStandaloneYtDlp
)

$ErrorActionPreference = 'Stop'
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
            $vcvarsCandidates = @(
                'D:\VS\BuildTools\VC\Auxiliary\Build\vcvars64.bat',
                "${env:ProgramFiles}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
                "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
                "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
            )
            $vcvars = $vcvarsCandidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
            if (-not $vcvars) {
                throw 'MSVC link.exe not found on PATH and vcvars64.bat could not be located.'
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
    $ytDlpUrl = 'https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe'
    Write-Host "Downloading standalone yt-dlp.exe from $ytDlpUrl..."
    $prevProgress = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $ytDlpUrl -OutFile $stagedYtDlp
    } finally {
        $ProgressPreference = $prevProgress
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
