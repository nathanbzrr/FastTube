<#
.SYNOPSIS
    Mede de forma nao intrusiva um processo existente no Windows (memoria privada/residente, CPU e GPU).

.DESCRIPTION
    Coleta amostras periodicas de um processo em execucao (por padrao `fasttube`) sem interrompe-lo,
    reinicia-lo ou modificar executaveis em disco.

    Metricas coletadas por amostra e resumidas (Min, Media, Mediana, P95, Max, Ultima):
    - Memoria privada residente (WorkingSetPrivate via WMI/CIM PerfProc, equivalente ao Gerenciador de Tarefas)
    - Memoria residente total (WorkingSet64) e Pico de Working Set (PeakWorkingSet64)
    - Memoria privada comitada (PrivateMemorySize64 / Private Bytes)
    - CPU (% total da maquina normalizado pelos nucleos logicos, % equivalente a 1 nucleo, User/Kernel, Threads, Handles)
    - GPU quando disponivel via contadores WDDM (utilizacao total, 3D/Compute, Video Decode/Codec, Copy, VRAM dedicada e compartilhada)
    - Subprocessos filhos ativos durante a amostra (ex.: `yt-dlp.exe`)

    Como o FastTube nao expoe telemetria externa do estado interno da UI/player, o cenario
    (`ocioso`, `pausado`, `reproducao`, `pesquisa`, `nao-informado`, etc.) deve ser informado
    manualmente pelo operador via `-Scenario` (ou `-PromptScenario`).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [Alias('Pid', 'Id')]
    [int[]]$ProcessId,

    [Parameter(Mandatory = $false)]
    [string]$ProcessName = 'fasttube',

    [Parameter(Mandatory = $false)]
    [ValidateSet('nao-informado', 'ocioso', 'pausado', 'reproducao', 'pesquisa', 'comparacao-navegador', 'outro')]
    [string]$Scenario = 'nao-informado',

    [Parameter(Mandatory = $false)]
    [string]$CustomScenarioLabel = '',

    [Parameter(Mandatory = $false)]
    [switch]$PromptScenario,

    [Parameter(Mandatory = $false)]
    [ValidateRange(1, 3600)]
    [int]$SampleCount = 10,

    [Parameter(Mandatory = $false)]
    [ValidateRange(0.2, 60.0)]
    [double]$IntervalSeconds = 1.0,

    [Parameter(Mandatory = $false)]
    [bool]$IncludeChildren = $true,

    [Parameter(Mandatory = $false)]
    [switch]$AggregateMatchingProcesses,

    [Parameter(Mandatory = $false)]
    [string]$VideoUrl = '',

    [Parameter(Mandatory = $false)]
    [string]$Resolution = '',

    [Parameter(Mandatory = $false)]
    [string]$Notes = '',

    [Parameter(Mandatory = $false)]
    [string]$OutputJson = '',

    [Parameter(Mandatory = $false)]
    [string]$OutputCsv = '',

    [Parameter(Mandatory = $false)]
    [string]$OutputMarkdown = '',

    [Parameter(Mandatory = $false)]
    [switch]$AppendMarkdown,

    [Parameter(Mandatory = $false)]
    [switch]$AsJson,

    [Parameter(Mandatory = $false)]
    [switch]$Quiet
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertTo-MiB {
    param([Parameter(Mandatory = $false)][AllowNull()]$Bytes)
    if ($null -eq $Bytes) { return $null }
    return [math]::Round(([double]$Bytes / 1MB), 2)
}

function Get-MetricStats {
    param(
        [Parameter(Mandatory = $false)]
        [object[]]$Values
    )
    $valid = @($Values | Where-Object { $null -ne $_ -and -not [double]::IsNaN($_) })
    if ($valid.Count -eq 0) {
        return [PSCustomObject]@{
            Count  = 0
            Min    = $null
            Avg    = $null
            Median = $null
            P95    = $null
            Max    = $null
            Last   = $null
        }
    }
    $sorted = @($valid | Sort-Object)
    $count = $sorted.Count
    $sum = ($sorted | Measure-Object -Sum).Sum
    $avg = $sum / $count

    if ($count % 2 -eq 1) {
        $median = $sorted[[int][math]::Floor($count / 2)]
    } else {
        $upper = [int]($count / 2)
        $median = ($sorted[$upper - 1] + $sorted[$upper]) / 2.0
    }

    $p95Index = [int][math]::Ceiling(0.95 * $count) - 1
    if ($p95Index -lt 0) { $p95Index = 0 }
    if ($p95Index -ge $count) { $p95Index = $count - 1 }
    $p95 = $sorted[$p95Index]

    return [PSCustomObject]@{
        Count  = $count
        Min    = [math]::Round([double]$sorted[0], 2)
        Avg    = [math]::Round([double]$avg, 2)
        Median = [math]::Round([double]$median, 2)
        P95    = [math]::Round([double]$p95, 2)
        Max    = [math]::Round([double]$sorted[$count - 1], 2)
        Last   = [math]::Round([double]$valid[$valid.Count - 1], 2)
    }
}

function Get-DescendantProcessIds {
    param(
        [Parameter(Mandatory = $true)]
        [int[]]$RootPids
    )
    $rootSet = [System.Collections.Generic.HashSet[int]]::new()
    $visited = [System.Collections.Generic.HashSet[int]]::new()
    $frontier = [System.Collections.Generic.List[int]]::new()
    foreach ($id in $RootPids) {
        [void]$rootSet.Add($id)
        [void]$visited.Add($id)
        $frontier.Add($id)
    }
    $descendants = [System.Collections.Generic.List[object]]::new()

    while ($frontier.Count -gt 0) {
        $parentFilter = ($frontier | ForEach-Object { "ParentProcessId = $_" }) -join ' OR '
        $frontier.Clear()
        $children = @(Get-CimInstance Win32_Process -Filter $parentFilter -Property ProcessId, ParentProcessId, Name -ErrorAction SilentlyContinue)
        foreach ($proc in $children) {
            $childPid = [int]$proc.ProcessId
            if (-not $visited.Contains($childPid)) {
                [void]$visited.Add($childPid)
                if (-not $rootSet.Contains($childPid)) {
                    $descendants.Add($proc)
                }
                $frontier.Add($childPid)
            }
        }
    }
    return @($descendants)
}

if ($PromptScenario) {
    Write-Host "Selecione o cenario atual do processo antes de iniciar a coleta:" -ForegroundColor Cyan
    Write-Host "  [1] ocioso      (janela aberta, sem video carregado)"
    Write-Host "  [2] pausado     (video carregado e pausado)"
    Write-Host "  [3] reproducao  (video em reproducao ativa)"
    Write-Host "  [4] pesquisa    (pesquisa ou consulta de resolucoes em andamento)"
    Write-Host "  [5] nao-informado / outro"
    $choice = Read-Host "Opcao (1-5)"
    switch ($choice.Trim()) {
        '1' { $Scenario = 'ocioso' }
        '2' { $Scenario = 'pausado' }
        '3' { $Scenario = 'reproducao' }
        '4' { $Scenario = 'pesquisa' }
        default {
            if ($Scenario -eq 'nao-informado') {
                $Scenario = 'nao-informado'
            }
        }
    }
}

$effectiveScenario = if (-not [string]::IsNullOrWhiteSpace($CustomScenarioLabel)) {
    $CustomScenarioLabel.Trim()
} else {
    $Scenario
}

$targetProcesses = @()
if ($null -ne $ProcessId -and $ProcessId.Count -gt 0) {
    foreach ($id in $ProcessId) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($null -eq $p) {
            throw "Processo com PID $id nao encontrado em execucao."
        }
        $targetProcesses += $p
    }
} else {
    $found = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Sort-Object Id)
    if ($found.Count -eq 0) {
        throw "Nenhum processo com nome '$ProcessName' esta em execucao no momento."
    }
    if ($found.Count -gt 1 -and -not $AggregateMatchingProcesses) {
        $pidList = ($found | ForEach-Object { "$($_.Id) ($($_.ProcessName))" }) -join ', '
        throw "Multiplos processos encontrados para '$ProcessName': $pidList. Especifique -ProcessId <PID> ou use -AggregateMatchingProcesses para agregar todos."
    }
    $targetProcesses = $found
}

$targetPids = @($targetProcesses | ForEach-Object { [int]$_.Id })
$primaryProc = $targetProcesses[0]
$executablePath = $null
$startTimeIso = $null
try {
    $executablePath = $primaryProc.Path
} catch {
    $executablePath = $null
}
try {
    if ($null -ne $primaryProc.StartTime) {
        $startTimeIso = $primaryProc.StartTime.ToString('o')
    }
} catch {
    $startTimeIso = $null
}

$logicalProcessors = [Environment]::ProcessorCount
$osInfo = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
$cpuInfo = @(Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue)
$gpuControllers = @(Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue)

$environmentMetadata = [PSCustomObject]@{
    MachineName           = [Environment]::MachineName
    OSCaption             = if ($osInfo) { $osInfo.Caption } else { [Environment]::OSVersion.VersionString }
    OSVersion             = if ($osInfo) { $osInfo.Version } else { [Environment]::OSVersion.Version.ToString() }
    OSBuild               = if ($osInfo) { $osInfo.BuildNumber } else { $null }
    TotalVisibleMemoryMiB = if ($osInfo) { [math]::Round($osInfo.TotalVisibleMemorySize / 1024.0, 1) } else { $null }
    ProcessorNames        = @(if ($cpuInfo.Count -gt 0) { $cpuInfo | ForEach-Object { $_.Name.Trim() } } else { @() })
    LogicalProcessors     = $logicalProcessors
    GpuControllers        = @(if ($gpuControllers.Count -gt 0) {
            $gpuControllers | ForEach-Object { "$($_.Name) (Driver $($_.DriverVersion))" }
        } else { @() })
}

if (-not $Quiet -and -not $AsJson) {
    Write-Host "Iniciando medicao nao intrusiva..." -ForegroundColor Cyan
    Write-Host ("  Alvo       : {0} (PID: {1})" -f $primaryProc.ProcessName, ($targetPids -join ', '))
    if ($executablePath) {
        Write-Host ("  Executavel : {0}" -f $executablePath)
    }
    Write-Host ("  Cenario    : {0}" -f $effectiveScenario)
    Write-Host ("  Amostras   : {0} (intervalo: {1}s)" -f $SampleCount, $IntervalSeconds)
}

# Baseline CPU times for first-sample delta
$prevCpuMap = @{}
foreach ($p in $targetProcesses) {
    $p.Refresh()
    $prevCpuMap[[int]$p.Id] = [PSCustomObject]@{
        TotalSec  = $p.TotalProcessorTime.TotalSeconds
        UserSec   = $p.UserProcessorTime.TotalSeconds
        KernelSec = $p.PrivilegedProcessorTime.TotalSeconds
    }
}
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$prevElapsedSec = $stopwatch.Elapsed.TotalSeconds
$lastIterationEndSec = $prevElapsedSec

$samples = [System.Collections.Generic.List[object]]::new()
$anyGpuCounterSeen = $false

for ($i = 1; $i -le $SampleCount; $i++) {
    if ($i -eq 1) {
        $initialWarmupMs = [int][math]::Min(250, [math]::Max(100, ($IntervalSeconds * 1000.0 * 0.25)))
        Start-Sleep -Milliseconds $initialWarmupMs
    } else {
        $queryOverheadSec = $lastIterationEndSec - $prevElapsedSec
        $remainingSec = [math]::Max(0.05, ($IntervalSeconds - $queryOverheadSec))
        Start-Sleep -Milliseconds ([int][math]::Round($remainingSec * 1000.0))
    }

    $sampleTimestamp = (Get-Date).ToString('o')
    $nowElapsedSec = $stopwatch.Elapsed.TotalSeconds
    $deltaWallSec = [math]::Max(0.001, ($nowElapsedSec - $prevElapsedSec))
    $prevElapsedSec = $nowElapsedSec

    $aliveTargets = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
    foreach ($id in $targetPids) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($null -ne $p) {
            $p.Refresh()
            $aliveTargets.Add($p)
        }
    }

    if ($aliveTargets.Count -eq 0) {
        Write-Warning "O processo monitorado encerrou antes da amostra $i."
        break
    }

    $workingSetBytes = [int64]0
    $peakWorkingSetBytes = [int64]0
    $privateBytes = [int64]0
    $pagedBytes = [int64]0
    $virtualBytes = [int64]0
    $threadCount = 0
    $handleCount = 0

    $deltaTotalCpuSec = 0.0
    $deltaUserCpuSec = 0.0
    $deltaKernelCpuSec = 0.0

    foreach ($p in $aliveTargets) {
        $pidInt = [int]$p.Id
        $workingSetBytes += [int64]$p.WorkingSet64
        $peakWorkingSetBytes += [int64]$p.PeakWorkingSet64
        $privateBytes += [int64]$p.PrivateMemorySize64
        $pagedBytes += [int64]$p.PagedMemorySize64
        $virtualBytes += [int64]$p.VirtualMemorySize64
        $threadCount += [int]$p.Threads.Count
        $handleCount += [int]$p.HandleCount

        $totSec = $p.TotalProcessorTime.TotalSeconds
        $usrSec = $p.UserProcessorTime.TotalSeconds
        $krnSec = $p.PrivilegedProcessorTime.TotalSeconds

        if ($prevCpuMap.ContainsKey($pidInt)) {
            $prev = $prevCpuMap[$pidInt]
            $deltaTotalCpuSec += [math]::Max(0.0, ($totSec - $prev.TotalSec))
            $deltaUserCpuSec += [math]::Max(0.0, ($usrSec - $prev.UserSec))
            $deltaKernelCpuSec += [math]::Max(0.0, ($krnSec - $prev.KernelSec))
        }
        $prevCpuMap[$pidInt] = [PSCustomObject]@{
            TotalSec  = $totSec
            UserSec   = $usrSec
            KernelSec = $krnSec
        }
    }

    $cpuSingleCorePercent = [math]::Round(($deltaTotalCpuSec / $deltaWallSec) * 100.0, 2)
    $cpuTotalPercent = [math]::Round((($deltaTotalCpuSec / $deltaWallSec) / $logicalProcessors) * 100.0, 2)
    $cpuUserPercent = [math]::Round((($deltaUserCpuSec / $deltaWallSec) / $logicalProcessors) * 100.0, 2)
    $cpuKernelPercent = [math]::Round((($deltaKernelCpuSec / $deltaWallSec) / $logicalProcessors) * 100.0, 2)

    # Resident private memory (WorkingSetPrivate) via PerfProc
    $pidFilter = ($aliveTargets | ForEach-Object { "IDProcess = $($_.Id)" }) -join ' OR '
    $perfProcItems = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter $pidFilter -ErrorAction SilentlyContinue)
    $workingSetPrivateBytes = $null
    if ($perfProcItems.Count -gt 0) {
        $sumWsp = [int64]0
        foreach ($item in $perfProcItems) {
            if ($null -ne $item.WorkingSetPrivate) {
                $sumWsp += [int64]$item.WorkingSetPrivate
            }
        }
        $workingSetPrivateBytes = $sumWsp
    }

    $childCount = 0
    $childNames = @()
    $childWorkingSetBytes = [int64]0
    $childPrivateBytes = [int64]0
    $childWorkingSetPrivateBytes = [int64]0

    if ($IncludeChildren) {
        $descendants = @(Get-DescendantProcessIds -RootPids ($aliveTargets | ForEach-Object { [int]$_.Id }))
        if ($descendants.Count -gt 0) {
            foreach ($desc in $descendants) {
                $cPid = [int]$desc.ProcessId
                $cp = Get-Process -Id $cPid -ErrorAction SilentlyContinue
                if ($null -ne $cp) {
                    $childCount++
                    $childNames += "$($cp.ProcessName)#$cPid"
                    $childWorkingSetBytes += [int64]$cp.WorkingSet64
                    $childPrivateBytes += [int64]$cp.PrivateMemorySize64
                }
            }
            if ($childCount -gt 0) {
                $childPidFilter = ($descendants | ForEach-Object { "IDProcess = $($_.ProcessId)" }) -join ' OR '
                $childPerfItems = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter $childPidFilter -ErrorAction SilentlyContinue)
                foreach ($cItem in $childPerfItems) {
                    if ($null -ne $cItem.WorkingSetPrivate) {
                        $childWorkingSetPrivateBytes += [int64]$cItem.WorkingSetPrivate
                    }
                }
            }
        }
    }

    $gpuFilter = ($aliveTargets | ForEach-Object { "Name LIKE 'pid_$($_.Id)_%'" }) -join ' OR '
    $gpuPidPattern = '^pid_(' + (($aliveTargets | ForEach-Object { [string]$_.Id }) -join '|') + ')_'
    $gpuEngines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -Filter $gpuFilter -ErrorAction SilentlyContinue | Where-Object { $_.Name -match $gpuPidPattern })
    $gpuMemItems = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -Filter $gpuFilter -ErrorAction SilentlyContinue | Where-Object { $_.Name -match $gpuPidPattern })

    $gpuSampleAvailable = ($gpuEngines.Count -gt 0 -or $gpuMemItems.Count -gt 0)
    if ($gpuSampleAvailable) {
        $anyGpuCounterSeen = $true
    }

    $gpuEngineTotalPercent = $null
    $gpuEngineMaxPercent = $null
    $gpu3DPercent = $null
    $gpuVideoCodecPercent = $null
    $gpuCopyPercent = $null
    $gpuActiveEngines = @{}

    if ($gpuEngines.Count -gt 0) {
        $totalUtil = 0.0
        $maxUtil = 0.0
        $util3D = 0.0
        $utilVideo = 0.0
        $utilCopy = 0.0

        foreach ($eng in $gpuEngines) {
            $u = [double]$eng.UtilizationPercentage
            $totalUtil += $u
            if ($u -gt $maxUtil) { $maxUtil = $u }

            $engName = [string]$eng.Name
            $engType = if ($engName -match 'engtype_(.+)$') { $Matches[1] } else { $engName }

            if ($u -gt 0) {
                if (-not $gpuActiveEngines.ContainsKey($engType)) {
                    $gpuActiveEngines[$engType] = 0.0
                }
                $gpuActiveEngines[$engType] += $u
            }

            if ($engType -match '^(3D|Compute|Graphics)') {
                $util3D += $u
            } elseif ($engType -match '(Video|Codec|Decode|Encode|VPE|JPEG)') {
                $utilVideo += $u
            } elseif ($engType -match 'Copy') {
                $utilCopy += $u
            }
        }

        $gpuEngineTotalPercent = [math]::Round($totalUtil, 2)
        $gpuEngineMaxPercent = [math]::Round($maxUtil, 2)
        $gpu3DPercent = [math]::Round($util3D, 2)
        $gpuVideoCodecPercent = [math]::Round($utilVideo, 2)
        $gpuCopyPercent = [math]::Round($utilCopy, 2)
    }

    $gpuDedicatedBytes = $null
    $gpuSharedBytes = $null
    $gpuLocalBytes = $null

    if ($gpuMemItems.Count -gt 0) {
        $dedSum = [int64]0
        $shrSum = [int64]0
        $locSum = [int64]0
        foreach ($m in $gpuMemItems) {
            if ($null -ne $m.DedicatedUsage) { $dedSum += [int64]$m.DedicatedUsage }
            if ($null -ne $m.SharedUsage) { $shrSum += [int64]$m.SharedUsage }
            if ($null -ne $m.LocalUsage) { $locSum += [int64]$m.LocalUsage }
        }
        $gpuDedicatedBytes = $dedSum
        $gpuSharedBytes = $shrSum
        $gpuLocalBytes = $locSum
    }

    $sampleObj = [PSCustomObject]@{
        SampleIndex                = $i
        Timestamp                  = $sampleTimestamp
        ElapsedSeconds             = [math]::Round($nowElapsedSec, 3)
        IntervalDeltaSeconds       = [math]::Round($deltaWallSec, 3)
        ProcessCount               = $aliveTargets.Count
        WorkingSetPrivateBytes     = $workingSetPrivateBytes
        WorkingSetPrivateMiB       = ConvertTo-MiB $workingSetPrivateBytes
        WorkingSetBytes            = $workingSetBytes
        WorkingSetMiB              = ConvertTo-MiB $workingSetBytes
        PeakWorkingSetBytes        = $peakWorkingSetBytes
        PeakWorkingSetMiB          = ConvertTo-MiB $peakWorkingSetBytes
        PrivateBytes               = $privateBytes
        PrivateMiB                 = ConvertTo-MiB $privateBytes
        PagedBytes                 = $pagedBytes
        PagedMiB                   = ConvertTo-MiB $pagedBytes
        VirtualBytes               = $virtualBytes
        VirtualMiB                 = ConvertTo-MiB $virtualBytes
        CpuTotalPercent            = $cpuTotalPercent
        CpuSingleCorePercent       = $cpuSingleCorePercent
        CpuUserPercent             = $cpuUserPercent
        CpuKernelPercent           = $cpuKernelPercent
        ThreadCount                = $threadCount
        HandleCount                = $handleCount
        GpuAvailable               = $gpuSampleAvailable
        GpuEngineTotalPercent      = $gpuEngineTotalPercent
        GpuEngineMaxPercent        = $gpuEngineMaxPercent
        Gpu3DPercent               = $gpu3DPercent
        GpuVideoCodecPercent       = $gpuVideoCodecPercent
        GpuCopyPercent             = $gpuCopyPercent
        GpuActiveEngines           = [PSCustomObject]$gpuActiveEngines
        GpuDedicatedBytes          = $gpuDedicatedBytes
        GpuDedicatedMiB            = ConvertTo-MiB $gpuDedicatedBytes
        GpuSharedBytes             = $gpuSharedBytes
        GpuSharedMiB               = ConvertTo-MiB $gpuSharedBytes
        GpuLocalBytes              = $gpuLocalBytes
        GpuLocalMiB                = ConvertTo-MiB $gpuLocalBytes
        ChildProcessCount          = $childCount
        ChildProcessNames          = ($childNames -join ', ')
        ChildWorkingSetPrivateMiB  = ConvertTo-MiB $childWorkingSetPrivateBytes
        ChildWorkingSetMiB         = ConvertTo-MiB $childWorkingSetBytes
        ChildPrivateMiB            = ConvertTo-MiB $childPrivateBytes
    }

    $samples.Add($sampleObj)
    $lastIterationEndSec = $stopwatch.Elapsed.TotalSeconds
}

if ($samples.Count -eq 0) {
    throw "Nenhuma amostra pode ser coletada."
}

$summary = [PSCustomObject]@{
    WorkingSetPrivateMiB  = Get-MetricStats -Values @($samples | ForEach-Object { $_.WorkingSetPrivateMiB })
    WorkingSetMiB         = Get-MetricStats -Values @($samples | ForEach-Object { $_.WorkingSetMiB })
    PeakWorkingSetMiB     = Get-MetricStats -Values @($samples | ForEach-Object { $_.PeakWorkingSetMiB })
    PrivateMiB            = Get-MetricStats -Values @($samples | ForEach-Object { $_.PrivateMiB })
    CpuTotalPercent       = Get-MetricStats -Values @($samples | ForEach-Object { $_.CpuTotalPercent })
    CpuSingleCorePercent  = Get-MetricStats -Values @($samples | ForEach-Object { $_.CpuSingleCorePercent })
    ThreadCount           = Get-MetricStats -Values @($samples | ForEach-Object { [double]$_.ThreadCount })
    HandleCount           = Get-MetricStats -Values @($samples | ForEach-Object { [double]$_.HandleCount })
    GpuAvailable          = $anyGpuCounterSeen
    GpuEngineTotalPercent = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuEngineTotalPercent }) } else { $null }
    Gpu3DPercent          = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.Gpu3DPercent }) } else { $null }
    GpuVideoCodecPercent  = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuVideoCodecPercent }) } else { $null }
    GpuDedicatedMiB       = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuDedicatedMiB }) } else { $null }
    GpuSharedMiB          = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuSharedMiB }) } else { $null }
    MaxChildProcessCount  = ($samples | Measure-Object -Property ChildProcessCount -Maximum).Maximum
}

$report = [PSCustomObject]@{
    SchemaVersion   = '1.0'
    CollectedAt     = (Get-Date).ToString('o')
    Scenario        = $effectiveScenario
    ScenarioSource  = 'manual'
    VideoUrl        = $VideoUrl
    Resolution      = $Resolution
    Notes           = $Notes
    Target          = [PSCustomObject]@{
        ProcessName        = $primaryProc.ProcessName
        ProcessIds         = $targetPids
        ExecutablePath     = $executablePath
        ProcessStartTime   = $startTimeIso
        AggregatedMatching = [bool]$AggregateMatchingProcesses
        IncludeChildren    = [bool]$IncludeChildren
    }
    Sampling        = [PSCustomObject]@{
        RequestedSamples = $SampleCount
        CollectedSamples = $samples.Count
        IntervalSeconds  = $IntervalSeconds
        TotalDurationSec = [math]::Round($stopwatch.Elapsed.TotalSeconds, 2)
    }
    Environment     = $environmentMetadata
    Summary         = $summary
    Samples         = @($samples)
}

if (-not [string]::IsNullOrWhiteSpace($OutputJson)) {
    $parentDir = Split-Path -Parent $OutputJson
    if (-not [string]::IsNullOrWhiteSpace($parentDir) -and -not (Test-Path $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -Path $OutputJson -Encoding UTF8
}

if (-not [string]::IsNullOrWhiteSpace($OutputCsv)) {
    $parentDir = Split-Path -Parent $OutputCsv
    if (-not [string]::IsNullOrWhiteSpace($parentDir) -and -not (Test-Path $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }
    $samples | Export-Csv -Path $OutputCsv -NoTypeInformation -Encoding UTF8
}

if (-not [string]::IsNullOrWhiteSpace($OutputMarkdown)) {
    $parentDir = Split-Path -Parent $OutputMarkdown
    if (-not [string]::IsNullOrWhiteSpace($parentDir) -and -not (Test-Path $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }

    $mdLines = @(
        "### Medicao: ``$effectiveScenario`` ($($report.CollectedAt))"
        ""
        "- **Processo**: ``$($report.Target.ProcessName)`` (PID: ``$($report.Target.ProcessIds -join ', ')``)"
        "- **Executavel**: ``$(if ($executablePath) { $executablePath } else { 'N/D' })``"
        "- **Amostragem**: $($report.Sampling.CollectedSamples) amostras a cada $($report.Sampling.IntervalSeconds)s (duracao: $($report.Sampling.TotalDurationSec)s)"
    )
    if (-not [string]::IsNullOrWhiteSpace($VideoUrl)) {
        $mdLines += "- **Video**: ``$VideoUrl``"
    }
    if (-not [string]::IsNullOrWhiteSpace($Resolution)) {
        $mdLines += "- **Resolucao**: ``$Resolution``"
    }
    if (-not [string]::IsNullOrWhiteSpace($Notes)) {
        $mdLines += "- **Observacoes**: $Notes"
    }
    $mdLines += @(
        ""
        "| Metrica | Min | Media | Mediana | P95 | Max |"
        "| :--- | ---: | ---: | ---: | ---: | ---: |"
        "| Memoria Privada Residente (`WorkingSetPrivate`, MiB) | $($summary.WorkingSetPrivateMiB.Min) | $($summary.WorkingSetPrivateMiB.Avg) | $($summary.WorkingSetPrivateMiB.Median) | $($summary.WorkingSetPrivateMiB.P95) | $($summary.WorkingSetPrivateMiB.Max) |"
        "| Memoria Residente Total (`WorkingSet64`, MiB) | $($summary.WorkingSetMiB.Min) | $($summary.WorkingSetMiB.Avg) | $($summary.WorkingSetMiB.Median) | $($summary.WorkingSetMiB.P95) | $($summary.WorkingSetMiB.Max) |"
        "| Memoria Privada Comitada (`PrivateBytes`, MiB) | $($summary.PrivateMiB.Min) | $($summary.PrivateMiB.Avg) | $($summary.PrivateMiB.Median) | $($summary.PrivateMiB.P95) | $($summary.PrivateMiB.Max) |"
        "| CPU Total da Maquina (%) | $($summary.CpuTotalPercent.Min) | $($summary.CpuTotalPercent.Avg) | $($summary.CpuTotalPercent.Median) | $($summary.CpuTotalPercent.P95) | $($summary.CpuTotalPercent.Max) |"
        "| CPU Equivalente 1 Nucleo (%) | $($summary.CpuSingleCorePercent.Min) | $($summary.CpuSingleCorePercent.Avg) | $($summary.CpuSingleCorePercent.Median) | $($summary.CpuSingleCorePercent.P95) | $($summary.CpuSingleCorePercent.Max) |"
        "| GPU Total Engines (%) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Max } else { 'N/D' }) |"
        "| GPU Video Codec/Decode (%) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Max } else { 'N/D' }) |"
        "| GPU 3D/Compute (%) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Max } else { 'N/D' }) |"
        "| GPU VRAM Dedicada (MiB) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Max } else { 'N/D' }) |"
        "| GPU VRAM Compartilhada (MiB) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Max } else { 'N/D' }) |"
        ""
    )
    $mdContent = ($mdLines -join [Environment]::NewLine) + [Environment]::NewLine
    if ($AppendMarkdown -and (Test-Path $OutputMarkdown)) {
        Add-Content -Path $OutputMarkdown -Value $mdContent -Encoding UTF8
    } else {
        Set-Content -Path $OutputMarkdown -Value $mdContent -Encoding UTF8
    }
}

if ($AsJson) {
    $report | ConvertTo-Json -Depth 8
} else {
    if (-not $Quiet) {
        Write-Host ""
        Write-Host "Resumo da Coleta ($($report.Sampling.CollectedSamples) amostras, $($report.Sampling.TotalDurationSec)s):" -ForegroundColor Green
        Write-Host ("  Memoria Privada Residente (WorkingSetPrivate) : media {0} MiB (min {1} / max {2} MiB)" -f $summary.WorkingSetPrivateMiB.Avg, $summary.WorkingSetPrivateMiB.Min, $summary.WorkingSetPrivateMiB.Max)
        Write-Host ("  Memoria Residente Total (WorkingSet64)        : media {0} MiB (min {1} / max {2} MiB)" -f $summary.WorkingSetMiB.Avg, $summary.WorkingSetMiB.Min, $summary.WorkingSetMiB.Max)
        Write-Host ("  Memoria Privada Comitada (PrivateBytes)       : media {0} MiB (min {1} / max {2} MiB)" -f $summary.PrivateMiB.Avg, $summary.PrivateMiB.Min, $summary.PrivateMiB.Max)
        Write-Host ("  CPU Total ({0} nucleos logicos)               : media {1}% (min {2}% / max {3}%)" -f $logicalProcessors, $summary.CpuTotalPercent.Avg, $summary.CpuTotalPercent.Min, $summary.CpuTotalPercent.Max)
        if ($summary.GpuAvailable) {
            Write-Host ("  GPU Utilizacao Total / Video Codec            : media {0}% (max {1}%) / Video Codec media {2}% (max {3}%)" -f $summary.GpuEngineTotalPercent.Avg, $summary.GpuEngineTotalPercent.Max, $summary.GpuVideoCodecPercent.Avg, $summary.GpuVideoCodecPercent.Max)
            Write-Host ("  GPU Memoria Dedicada / Compartilhada          : media {0} MiB / {1} MiB" -f $summary.GpuDedicatedMiB.Avg, $summary.GpuSharedMiB.Avg)
        } else {
            Write-Host "  GPU                                           : Contadores WDDM indisponiveis para o processo"
        }
    }
    return $report
}
