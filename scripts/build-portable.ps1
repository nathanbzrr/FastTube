[CmdletBinding()]
param(
    [Parameter(Mandatory)][string] $LibMpvDirectory,
    [Parameter(Mandatory)][string] $BundleDirectory,
    [string] $YtDlpPath,
    [string] $TargetDirectory,
    [string] $TargetTriple
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'portable-package.psm1') -Force
if (-not $TargetDirectory) {
    $TargetDirectory = Join-Path (Split-Path -Parent $PSScriptRoot) 'target\portable-build'
}

$mpvDlls = @(Get-PortableMpvDlls -Directory $LibMpvDirectory)
$ytDlp = Resolve-PortableYtDlp -Path $YtDlpPath
$repoRoot = Split-Path -Parent $PSScriptRoot
$standardTarget = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target'))
$targetPath = if ([IO.Path]::IsPathRooted($TargetDirectory)) { $TargetDirectory } else { Join-Path $repoRoot $TargetDirectory }
$resolvedTarget = [IO.Path]::GetFullPath($targetPath)
if ($resolvedTarget.TrimEnd('\') -ieq $standardTarget.TrimEnd('\')) {
    throw "TargetDirectory deve ser isolado; o diretório padrão do Cargo não pode ser usado: $standardTarget"
}
$cargoArgs = @('build', '--release', '--locked', '--manifest-path', (Join-Path $repoRoot 'Cargo.toml'), '--target-dir', $TargetDirectory)
if ($TargetTriple) { $cargoArgs += @('--target', $TargetTriple) }

Push-Location $repoRoot
try {
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo build falhou com código $LASTEXITCODE" }
} finally {
    Pop-Location
}

if ($TargetTriple) {
    $executable = Join-Path $TargetDirectory (Join-Path $TargetTriple 'release\fasttube.exe')
} else {
    $executable = Join-Path $TargetDirectory 'release\fasttube.exe'
}
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "O build não produziu o executável Windows esperado: $executable"
}

$distributionFiles = @(
    (Join-Path $repoRoot 'README.md'),
    (Join-Path $repoRoot 'LICENSE-MIT'),
    (Join-Path $repoRoot 'LICENSE-APACHE'),
    (Join-Path $repoRoot 'THIRD_PARTY_NOTICES.md')
)
New-PortableBundle -Executable $executable -YtDlp $ytDlp -MpvDlls $mpvDlls -AdditionalFiles $distributionFiles -Destination $BundleDirectory
Write-Host "Bundle portátil criado em: $((Resolve-Path -LiteralPath $BundleDirectory).Path)"
