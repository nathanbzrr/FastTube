$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'portable-package.psm1') -Force

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

    $fakePath = Join-Path $root 'fake-path'
    New-Item -ItemType Directory -Path $fakePath | Out-Null
    $fakeYtFromPath = Join-Path $fakePath 'yt-dlp.exe'
    Set-Content -LiteralPath $fakeYtFromPath -Value 'fake PATH yt-dlp'
    $previousPath = $env:PATH
    try {
        $env:PATH = "$fakePath;$previousPath"
        if ((Resolve-PortableYtDlp) -ne $fakeYtFromPath) { throw 'yt-dlp disponível no PATH não foi resolvido.' }
    } finally {
        $env:PATH = $previousPath
    }

    $dlls = @(Get-PortableMpvDlls -Directory $dist)
    if ($dlls.Count -ne 2) { throw "Esperadas 2 DLLs, obtidas $($dlls.Count)." }
    New-PortableBundle -Executable $exe -YtDlp (Resolve-PortableYtDlp -Path $yt) -MpvDlls $dlls -Destination $bundle
    $expected = @('fasttube.exe', 'yt-dlp.exe', 'libmpv-2.dll', 'helper.dll') | Sort-Object
    $actual = @(Get-ChildItem -LiteralPath $bundle -File | Select-Object -ExpandProperty Name | Sort-Object)
    if (Compare-Object $expected $actual) { throw "Conteúdo incorreto no bundle: $($actual -join ', ')" }
    if (Test-Path -LiteralPath (Join-Path $bundle 'user-data.json')) { throw 'Arquivo de usuário foi incluído.' }
    if ((Get-Content -Raw -LiteralPath (Join-Path $bundle 'yt-dlp.exe')).Trim() -ne 'fake yt-dlp') { throw 'yt-dlp explícito não foi copiado.' }

    $missing = Join-Path $root 'missing'
    New-Item -ItemType Directory -Path $missing | Out-Null
    try { Get-PortableMpvDlls -Directory $missing | Out-Null; throw 'Distribuição sem libmpv foi aceita.' }
    catch { if ($_.Exception.Message -eq 'Distribuição sem libmpv foi aceita.') { throw } }
    try { Resolve-PortableYtDlp -Path (Join-Path $root 'absent.exe') | Out-Null; throw 'yt-dlp ausente foi aceito.' }
    catch { if ($_.Exception.Message -eq 'yt-dlp ausente foi aceito.') { throw } }
    try { New-PortableBundle -Executable $exe -YtDlp $yt -MpvDlls $dlls -Destination $bundle; throw 'Destino não vazio foi sobrescrito.' }
    catch { if ($_.Exception.Message -eq 'Destino não vazio foi sobrescrito.') { throw } }

    Write-Host 'Portable packaging fixture checks passed.'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
