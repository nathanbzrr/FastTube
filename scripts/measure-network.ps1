<#
.SYNOPSIS
 Mede trafego total por interface, sem captura de pacotes.
.DESCRIPTION
 Inclui outros processos, nao e por PID e nao pode ser atribuido ao FastTube.
 Metadados de cenario sao manuais. Por padrao mede as interfaces ativas.
.EXAMPLE
 .\scripts\measure-network.ps1 -InterfaceName 'Ethernet' -Scenario 'reproducao' -OutputJson .\rede.json -OutputCsv .\rede.csv
#>
[CmdletBinding()]
param(
    [string[]]$InterfaceName = @(),
    [ValidateRange(1, 3600)][int]$SampleCount = 10,
    [ValidateRange(0.1, 3600)][double]$IntervalSeconds = 1,
    [string]$Scenario = 'nao-informado',
    [string]$CustomScenarioLabel = '',
    [string]$VideoUrl = '',
    [string]$Resolution = '',
    [string]$Notes = '',
    [string]$OutputJson = '',
    [string]$OutputCsv = '',
    [switch]$Quiet
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($OutputJson -and $OutputCsv -and [System.IO.Path]::GetFullPath($OutputJson) -eq [System.IO.Path]::GetFullPath($OutputCsv)) {
    throw 'Os caminhos JSON e CSV devem ser diferentes.'
}
$scope = 'Trafego total da interface, incluindo outros processos; nao e por PID e nao pode ser atribuido ao FastTube.'
if (-not $Quiet) { Write-Warning $scope }
try { $interfaces = @([System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces()) } catch {
    throw ('Interfaces indisponiveis: ' + $_.Exception.Message)
}
if ($InterfaceName.Count -gt 0) {
    $selected = @($interfaces | Where-Object { $InterfaceName -contains $_.Name })
    foreach ($name in $InterfaceName) {
        if (@($selected | Where-Object { $_.Name -eq $name }).Count -ne 1) { throw ('Interface inexistente ou ambigua: ' + $name) }
    }
} else {
    $selected = @($interfaces | Where-Object { $_.OperationalStatus -eq 'Up' })
}
if ($selected.Count -eq 0) { throw 'Nenhuma interface ativa disponivel.' }
$clock = [System.Diagnostics.Stopwatch]::StartNew()
function Read-InterfaceCounters {
    param($Interface, $Clock)
    try {
        $statistics = $Interface.GetIPStatistics()
        $ticks = $Clock.ElapsedTicks
        $timestamp = [DateTime]::UtcNow.ToString('o')
        $received = [long]$statistics.BytesReceived
        $sent = [long]$statistics.BytesSent
        if ($received -lt 0 -or $sent -lt 0) { throw 'Contador negativo.' }
        [PSCustomObject]@{ Ticks = $ticks; TimestampUtc = $timestamp; Received = $received; Sent = $sent }
    } catch {
        throw ('Falha ao ler interface ' + $Interface.Name + ': ' + $_.Exception.Message)
    }
}
$previous = @{}
foreach ($interface in $selected) { $previous[$interface.Id] = Read-InterfaceCounters $interface $clock }
$samples = New-Object 'System.Collections.Generic.List[object]'
for ($sampleNumber = 1; $sampleNumber -le $SampleCount; $sampleNumber++) {
    Start-Sleep -Milliseconds ([int][math]::Ceiling($IntervalSeconds * 1000))
    try { $currentInterfaces = @([System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces()) } catch {
        throw ('Falha ao atualizar interfaces: ' + $_.Exception.Message)
    }
    foreach ($interface in $selected) {
        $currentInterface = @($currentInterfaces | Where-Object { $_.Id -eq $interface.Id })
        if ($currentInterface.Count -ne 1) { throw ('Interface desapareceu: ' + $interface.Name) }
        $current = Read-InterfaceCounters $currentInterface[0] $clock
        $prior = $previous[$interface.Id]
        $elapsed = ($current.Ticks - $prior.Ticks) / [double][System.Diagnostics.Stopwatch]::Frequency
        if ($elapsed -le 0) { throw ('Intervalo invalido: ' + $interface.Name) }
        if ($current.Received -lt $prior.Received -or $current.Sent -lt $prior.Sent) {
            throw ('Contadores diminuiram, possivel reinicializacao: ' + $interface.Name)
        }
        $samples.Add([PSCustomObject]@{
            Sample = $sampleNumber
            InterfaceName = $interface.Name
            InterfaceId = $interface.Id
            StartUtc = $prior.TimestampUtc
            EndUtc = $current.TimestampUtc
            ElapsedSeconds = $elapsed
            ReceivedBytesStart = $prior.Received
            ReceivedBytesEnd = $current.Received
            SentBytesStart = $prior.Sent
            SentBytesEnd = $current.Sent
            ReceivedBytesPerSecond = ($current.Received - $prior.Received) / $elapsed
            SentBytesPerSecond = ($current.Sent - $prior.Sent) / $elapsed
            Scenario = $Scenario
            CustomScenarioLabel = $CustomScenarioLabel
            VideoUrl = $VideoUrl
            Resolution = $Resolution
            Notes = $Notes
            Scope = $scope
        })
        $previous[$interface.Id] = $current
    }
}
$result = [PSCustomObject]@{
    SchemaVersion = '1.0'
    Scope = $scope
    CounterSource = 'System.Net.NetworkInformation.NetworkInterface.GetIPStatistics'
    RequestedIntervalSeconds = $IntervalSeconds
    SampleCount = $SampleCount
    Scenario = $Scenario
    CustomScenarioLabel = $CustomScenarioLabel
    VideoUrl = $VideoUrl
    Resolution = $Resolution
    Notes = $Notes
    Samples = @($samples.ToArray())
}
if ($OutputJson) { $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputJson -Encoding UTF8 }
if ($OutputCsv) { $result.Samples | Export-Csv -LiteralPath $OutputCsv -NoTypeInformation -Encoding UTF8 }
$result
