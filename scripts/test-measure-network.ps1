<#
.SYNOPSIS
 Testa a medicao real sem alterar configuracoes de rede ou processos.
#>
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$scriptPath = Join-Path $PSScriptRoot 'measure-network.ps1'
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ('network-measure-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempDir | Out-Null
try {
    $available = @([System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces() | Where-Object { $_.OperationalStatus -eq 'Up' })
    if ($available.Count -eq 0) { throw 'Teste real indisponivel: nenhuma interface ativa.' }
    $interface = $available[0]
    $jsonPath = Join-Path $tempDir 'result.json'
    $csvPath = Join-Path $tempDir 'result.csv'
    Write-Host ('Testando contadores reais em: ' + $interface.Name)
    $result = & $scriptPath -InterfaceName $interface.Name -SampleCount 2 -IntervalSeconds 0.2 `
        -Scenario 'reproducao' -CustomScenarioLabel 'manual' -VideoUrl 'example' -Resolution '1080p' `
        -Notes 'teste' -OutputJson $jsonPath -OutputCsv $csvPath -Quiet
    if ($result.Samples.Count -ne 2) { throw 'Quantidade de amostras incorreta.' }
    if ($result.Scenario -ne 'reproducao' -or $result.CustomScenarioLabel -ne 'manual' -or
        $result.VideoUrl -ne 'example' -or $result.Resolution -ne '1080p' -or $result.Notes -ne 'teste') {
        throw 'Metadados manuais incorretos.'
    }
    if ($result.Scope -notmatch 'outros processos' -or $result.Scope -notmatch 'nao e por PID' -or
        $result.Scope -notmatch 'nao pode ser atribuido ao FastTube') { throw 'Escopo incorreto.' }
    foreach ($sample in $result.Samples) {
        if ($sample.InterfaceId -ne $interface.Id -or $sample.ElapsedSeconds -le 0 -or
            $sample.ReceivedBytesEnd -lt $sample.ReceivedBytesStart -or $sample.SentBytesEnd -lt $sample.SentBytesStart) {
            throw 'Identidade, intervalo ou contadores invalidos.'
        }
        $expectedReceived = ($sample.ReceivedBytesEnd - $sample.ReceivedBytesStart) / $sample.ElapsedSeconds
        $expectedSent = ($sample.SentBytesEnd - $sample.SentBytesStart) / $sample.ElapsedSeconds
        if ([math]::Abs($sample.ReceivedBytesPerSecond - $expectedReceived) -gt 0.000001 -or
            [math]::Abs($sample.SentBytesPerSecond - $expectedSent) -gt 0.000001) { throw 'Taxa incorreta.' }
    }
    $json = Get-Content -LiteralPath $jsonPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $csv = @(Import-Csv -LiteralPath $csvPath -Encoding UTF8)
    if ($json.Samples.Count -ne 2 -or $csv.Count -ne 2 -or $csv[0].Scenario -ne 'reproducao' -or
        $csv[0].Scope -ne $result.Scope -or $json.Samples[0].InterfaceId -ne $interface.Id) { throw 'Exportacao incorreta.' }
    $withoutExport = & $scriptPath -InterfaceName $interface.Name -SampleCount 1 -IntervalSeconds 0.1 -Quiet
    if ($withoutExport.Samples.Count -ne 1) { throw 'Medicao sem exportacao falhou.' }
    $failed = $false
    try { & $scriptPath -InterfaceName ('inexistente-' + [Guid]::NewGuid()) -SampleCount 1 -Quiet | Out-Null } catch { $failed = $true }
    if (-not $failed) { throw 'Interface inexistente deveria falhar.' }
    $failed = $false
    try { & $scriptPath -InterfaceName $interface.Name -OutputJson $jsonPath -OutputCsv $jsonPath -Quiet | Out-Null } catch { $failed = $true }
    if (-not $failed) { throw 'Caminhos iguais deveriam falhar.' }
    Write-Host 'OK: medicoes reais, calculos, metadados, exportacoes e erros validados.'
} finally {
    Remove-Item -LiteralPath $tempDir -Recurse -Force
}
