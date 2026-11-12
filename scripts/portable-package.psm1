Set-StrictMode -Version Latest

function Resolve-PortableYtDlp {
    param([string] $Path)

    if ($Path) {
        $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
        if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
            throw "yt-dlp precisa ser um arquivo: $Path"
        }
        return $resolved
    }

    $command = Get-Command yt-dlp -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $command) {
        throw 'yt-dlp ausente. Informe -YtDlpPath com o executável fornecido ou instale-o manualmente no PATH.'
    }
    return $command.Source
}

function Get-PortableMpvDlls {
    param([Parameter(Mandatory)][string] $Directory)

    $resolved = (Resolve-Path -LiteralPath $Directory -ErrorAction Stop).Path
    $dlls = @(Get-ChildItem -LiteralPath $resolved -File -Filter '*.dll' | Sort-Object Name)
    if (-not ($dlls.Name -contains 'libmpv-2.dll' -or $dlls.Name -contains 'mpv-2.dll')) {
        throw "A distribuição fornecida não contém libmpv-2.dll nem mpv-2.dll: $resolved"
    }
    return $dlls
}

function New-PortableBundle {
    param(
        [Parameter(Mandatory)][string] $Executable,
        [Parameter(Mandatory)][string] $YtDlp,
        [Parameter(Mandatory)][object[]] $MpvDlls,
        [Parameter(Mandatory)][string] $Destination
    )

    if (Test-Path -LiteralPath $Destination) {
        $existing = @(Get-ChildItem -LiteralPath $Destination -Force)
        if ($existing.Count -gt 0) {
            throw "O destino precisa estar vazio para evitar sobrescrever uma instalação: $Destination"
        }
    } else {
        New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    }

    Copy-Item -LiteralPath $Executable -Destination (Join-Path $Destination 'fasttube.exe')
    Copy-Item -LiteralPath $YtDlp -Destination (Join-Path $Destination 'yt-dlp.exe')
    foreach ($dll in $MpvDlls) {
        Copy-Item -LiteralPath $dll.FullName -Destination (Join-Path $Destination $dll.Name)
    }
}

Export-ModuleMember -Function Resolve-PortableYtDlp, Get-PortableMpvDlls, New-PortableBundle
