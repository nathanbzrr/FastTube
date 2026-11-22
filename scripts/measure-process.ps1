<#
.SYNOPSIS
    Non-intrusively measures a running Windows process and its child process tree (RAM, CPU, and GPU).

.DESCRIPTION
    Collects periodic samples from an existing process (default: `fasttube`) without interrupting,
    restarting, or modifying executables on disk.

    Metrics collected per sample and summarized (Min, Avg, Median, P95, Max, Last):
    - Resident private memory (WorkingSetPrivate via WMI/CIM PerfProc, matching Task Manager)
    - Total resident memory (WorkingSet64) and peak working set (PeakWorkingSet64)
    - Committed private memory (PrivateMemorySize64 / Private Bytes)
    - CPU (% total machine normalized by logical cores, % single-core equivalent, User/Kernel, Threads, Handles)
    - GPU utilization when available via WDDM counters (total, 3D/Compute, Video Decode/Codec, Copy, Dedicated/Shared VRAM)
    - Full recursive child process-tree aggregation (ChildProcessCount, TreeCpuPercent, TreeWorkingSetMB,
      TreePrivateMB, TreeWorkingSetPrivateMB, TreeGpuPercent) covering helper processes such as
      `yt-dlp.exe`, `ffmpeg.exe`, or multi-process browser workers.

    Dot-sourcing this script (`. .\scripts\measure-process.ps1`) loads the pure helper functions
    (`Get-ProcessTreePids`, `Aggregate-ProcessTreeSample`, `Parse-GpuEngineUtilization`, `Get-MetricStats`)
    without executing a live measurement.
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
    [switch]$IncludeChildProcesses,

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

function Get-ObjectPropertyValue {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string[]]$Names,
        [Parameter(Mandatory = $false)]$DefaultValue = $null
    )
    if ($null -eq $Object) { return $DefaultValue }
    if ($Object -is [System.Collections.IDictionary]) {
        foreach ($name in $Names) {
            if ($Object.Contains($name)) {
                return $Object[$name]
            }
        }
        return $DefaultValue
    }
    foreach ($name in $Names) {
        $prop = $Object.PSObject.Properties[$name]
        if ($null -ne $prop) {
            return $prop.Value
        }
    }
    return $DefaultValue
}

function Get-MetricStats {
    param(
        [Parameter(Mandatory = $false)]
        [AllowNull()]
        [object[]]$Values
    )
    $valid = @($Values | Where-Object { $null -ne $_ -and -not [double]::IsNaN([double]$_) })
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
    $sorted = @($valid | ForEach-Object { [double]$_ } | Sort-Object)
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

function Get-ProcessTreePids {
    <#
    .SYNOPSIS
        Recursively resolves all PIDs in a process tree from a (ProcessId, ParentProcessId) table.
    .DESCRIPTION
        Uses breadth-first traversal with a visited set to prevent infinite loops caused by
        Windows PID reuse or malformed parent cycles.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [int[]]$RootPids,

        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [object[]]$ProcessTable,

        [Parameter(Mandatory = $false)]
        [switch]$DescendantsOnly
    )

    $childrenByParent = [System.Collections.Generic.Dictionary[int, System.Collections.Generic.List[int]]]::new()
    foreach ($entry in @($ProcessTable)) {
        if ($null -eq $entry) { continue }
        $rawPid = Get-ObjectPropertyValue -Object $entry -Names @('ProcessId', 'Id')
        $rawParent = Get-ObjectPropertyValue -Object $entry -Names @('ParentProcessId')
        if ($null -eq $rawPid -or $null -eq $rawParent) { continue }

        $entryPid = [int]$rawPid
        $parentPid = [int]$rawParent
        if ($entryPid -le 0 -or $entryPid -eq $parentPid) { continue }

        if (-not $childrenByParent.ContainsKey($parentPid)) {
            $childrenByParent[$parentPid] = [System.Collections.Generic.List[int]]::new()
        }
        $childrenByParent[$parentPid].Add($entryPid)
    }

    $visited = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $result = [System.Collections.Generic.List[int]]::new()

    foreach ($rootPid in $RootPids) {
        if ($rootPid -le 0) { continue }
        if ($visited.Add($rootPid)) {
            $queue.Enqueue($rootPid)
            if (-not $DescendantsOnly) {
                $result.Add($rootPid)
            }
        }
    }

    while ($queue.Count -gt 0) {
        $current = $queue.Dequeue()
        if ($childrenByParent.ContainsKey($current)) {
            foreach ($childPid in $childrenByParent[$current]) {
                if ($visited.Add($childPid)) {
                    $result.Add($childPid)
                    $queue.Enqueue($childPid)
                }
            }
        }
    }

    return @($result.ToArray())
}

function Aggregate-ProcessTreeSample {
    <#
    .SYNOPSIS
        Aggregates CPU, memory, process counts, and child process names across root and descendant snapshots.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [int[]]$RootPids,

        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [object[]]$ProcessSnapshots,

        [Parameter(Mandatory = $false)]
        [hashtable]$PreviousCpuSeconds = @{},

        [Parameter(Mandatory = $false)]
        [double]$DeltaWallSeconds = 1.0,

        [Parameter(Mandatory = $false)]
        [int]$LogicalProcessors = [Environment]::ProcessorCount
    )

    $safeWallSec = [math]::Max(0.001, $DeltaWallSeconds)
    $safeCores = [math]::Max(1, $LogicalProcessors)

    $rootSet = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($rootPid in $RootPids) {
        [void]$rootSet.Add($rootPid)
    }

    $seenPids = [System.Collections.Generic.HashSet[int]]::new()
    $nextCpuMap = @{}
    if ($null -ne $PreviousCpuSeconds) {
        foreach ($key in $PreviousCpuSeconds.Keys) {
            $nextCpuMap[[int]$key] = [double]$PreviousCpuSeconds[$key]
        }
    }

    $rootCount = 0
    $childCount = 0
    $childNames = [System.Collections.Generic.List[string]]::new()

    $rootWsBytes = [int64]0
    $rootPrivateBytes = [int64]0
    $rootWspBytes = [int64]0
    $rootWspSeen = $false
    $rootCpuSec = 0.0
    $rootDeltaCpuSec = 0.0

    $childWsBytes = [int64]0
    $childPrivateBytes = [int64]0
    $childWspBytes = [int64]0
    $childWspSeen = $false
    $childCpuSec = 0.0
    $childDeltaCpuSec = 0.0

    foreach ($snap in @($ProcessSnapshots)) {
        if ($null -eq $snap) { continue }
        $rawPid = Get-ObjectPropertyValue -Object $snap -Names @('ProcessId', 'Id')
        if ($null -eq $rawPid) { continue }
        $procId = [int]$rawPid
        if (-not $seenPids.Add($procId)) { continue }

        $procName = [string](Get-ObjectPropertyValue -Object $snap -Names @('ProcessName', 'Name') -DefaultValue "pid_$procId")
        $wsBytes = [int64](Get-ObjectPropertyValue -Object $snap -Names @('WorkingSetBytes', 'WorkingSet64') -DefaultValue 0)
        $privBytes = [int64](Get-ObjectPropertyValue -Object $snap -Names @('PrivateBytes', 'PrivateMemorySize64') -DefaultValue 0)
        $wspRaw = Get-ObjectPropertyValue -Object $snap -Names @('WorkingSetPrivateBytes', 'WorkingSetPrivate') -DefaultValue $null
        $cpuSec = [double](Get-ObjectPropertyValue -Object $snap -Names @('TotalProcessorTimeSeconds', 'CpuSeconds') -DefaultValue 0.0)
        $isNewChild = [bool](Get-ObjectPropertyValue -Object $snap -Names @('IsNewChild') -DefaultValue $false)

        $deltaCpu = 0.0
        if ($null -ne $PreviousCpuSeconds -and $PreviousCpuSeconds.ContainsKey($procId)) {
            $deltaCpu = [math]::Max(0.0, ($cpuSec - [double]$PreviousCpuSeconds[$procId]))
        } elseif ($isNewChild -and -not $rootSet.Contains($procId)) {
            # Capture CPU burned by short-lived children spawned after the previous sample, bounded by wall interval.
            $deltaCpu = [math]::Min([math]::Max(0.0, $cpuSec), ($safeWallSec * $safeCores))
        }
        $nextCpuMap[$procId] = $cpuSec

        if ($rootSet.Contains($procId)) {
            $rootCount++
            $rootWsBytes += $wsBytes
            $rootPrivateBytes += $privBytes
            $rootCpuSec += $cpuSec
            $rootDeltaCpuSec += $deltaCpu
            if ($null -ne $wspRaw) {
                $rootWspBytes += [int64]$wspRaw
                $rootWspSeen = $true
            }
        } else {
            $childCount++
            $childNames.Add("$procName#$procId")
            $childWsBytes += $wsBytes
            $childPrivateBytes += $privBytes
            $childCpuSec += $cpuSec
            $childDeltaCpuSec += $deltaCpu
            if ($null -ne $wspRaw) {
                $childWspBytes += [int64]$wspRaw
                $childWspSeen = $true
            }
        }
    }

    $treeWsBytes = $rootWsBytes + $childWsBytes
    $treePrivateBytes = $rootPrivateBytes + $childPrivateBytes
    $treeWspBytes = if ($rootWspSeen -or $childWspSeen) { $rootWspBytes + $childWspBytes } else { $null }
    $treeCpuSec = $rootCpuSec + $childCpuSec
    $treeDeltaCpuSec = $rootDeltaCpuSec + $childDeltaCpuSec

    $treeCpuSingleCorePct = [math]::Round(($treeDeltaCpuSec / $safeWallSec) * 100.0, 2)
    $treeCpuPct = [math]::Round((($treeDeltaCpuSec / $safeWallSec) / $safeCores) * 100.0, 2)
    $childCpuPct = [math]::Round((($childDeltaCpuSec / $safeWallSec) / $safeCores) * 100.0, 2)

    return [PSCustomObject]@{
        RootProcessCount            = $rootCount
        ChildProcessCount           = $childCount
        TreeProcessCount            = $rootCount + $childCount
        ChildProcessNames           = @($childNames.ToArray())
        RootWorkingSetBytes         = $rootWsBytes
        RootPrivateBytes            = $rootPrivateBytes
        RootWorkingSetPrivateBytes  = if ($rootWspSeen) { $rootWspBytes } else { $null }
        ChildWorkingSetBytes        = $childWsBytes
        ChildPrivateBytes           = $childPrivateBytes
        ChildWorkingSetPrivateBytes = $childWspBytes
        TreeWorkingSetBytes         = $treeWsBytes
        TreeWorkingSetMB            = ConvertTo-MiB $treeWsBytes
        TreePrivateBytes            = $treePrivateBytes
        TreePrivateMB               = ConvertTo-MiB $treePrivateBytes
        TreeWorkingSetPrivateBytes  = $treeWspBytes
        TreeWorkingSetPrivateMB     = ConvertTo-MiB $treeWspBytes
        TreeCpuSeconds              = [math]::Round($treeCpuSec, 4)
        TreeDeltaCpuSeconds         = [math]::Round($treeDeltaCpuSec, 4)
        TreeCpuPercent              = $treeCpuPct
        TreeCpuSingleCorePercent    = $treeCpuSingleCorePct
        ChildDeltaCpuSeconds        = [math]::Round($childDeltaCpuSec, 4)
        ChildCpuPercent             = $childCpuPct
        NextCpuSeconds              = $nextCpuMap
    }
}

function Parse-GpuEngineUtilization {
    <#
    .SYNOPSIS
        Parses Windows GPU Engine utilization samples (Get-Counter or WMI PerfFormattedData) for a PID set.
    .DESCRIPTION
        Extracts PID and engine type from counter paths such as
        `\GPU Engine(pid_1234_luid_..._engtype_3D)\Utilization Percentage` or WMI instance names
        `pid_1234_luid_..._engtype_3D`. Returns Available=$false and TotalPercent=$null gracefully
        when counters are unavailable or no matching PID samples are present.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $false)]
        [AllowNull()]
        [AllowEmptyCollection()]
        [object[]]$CounterSamples,

        [Parameter(Mandatory = $true)]
        [int[]]$TargetPids
    )

    $emptyResult = [PSCustomObject]@{
        Available          = $false
        TotalPercent       = $null
        MaxEnginePercent   = $null
        Engine3DPercent    = $null
        VideoCodecPercent  = $null
        CopyPercent        = $null
        ActiveEngines      = [PSCustomObject]@{}
        PerPidPercent      = @{}
        MatchedSampleCount = 0
    }

    if ($null -eq $CounterSamples -or $null -eq $TargetPids) {
        return $emptyResult
    }
    $samplesList = @($CounterSamples)
    $targetPidList = @($TargetPids)
    if ($samplesList.Count -eq 0 -or $targetPidList.Count -eq 0) {
        return $emptyResult
    }

    $targetSet = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($targetPid in $TargetPids) {
        [void]$targetSet.Add($targetPid)
    }

    $totalUtil = 0.0
    $maxUtil = 0.0
    $util3D = 0.0
    $utilVideo = 0.0
    $utilCopy = 0.0
    $activeEngines = @{}
    $perPid = @{}
    $matchedCount = 0

    foreach ($sample in $samplesList) {
        if ($null -eq $sample) { continue }

        $rawLabel = [string](Get-ObjectPropertyValue -Object $sample -Names @('Path', 'InstanceName', 'Name') -DefaultValue '')
        if ([string]::IsNullOrWhiteSpace($rawLabel)) { continue }
        if ($rawLabel -notmatch 'pid_(\d+)_') { continue }

        $samplePid = [int]$Matches[1]
        if (-not $targetSet.Contains($samplePid)) { continue }

        $rawValue = Get-ObjectPropertyValue -Object $sample -Names @('CookedValue', 'UtilizationPercentage', 'Value') -DefaultValue $null
        if ($null -eq $rawValue) { continue }
        $u = [double]$rawValue
        if ([double]::IsNaN($u) -or [double]::IsInfinity($u)) { continue }
        if ($u -lt 0.0) { $u = 0.0 }

        $matchedCount++
        $totalUtil += $u
        if ($u -gt $maxUtil) { $maxUtil = $u }

        if (-not $perPid.ContainsKey($samplePid)) {
            $perPid[$samplePid] = 0.0
        }
        $perPid[$samplePid] += $u

        $engType = if ($rawLabel -match 'engtype_([^)\\]+)') { $Matches[1] } else { $rawLabel }

        if ($u -gt 0.0) {
            if (-not $activeEngines.ContainsKey($engType)) {
                $activeEngines[$engType] = 0.0
            }
            $activeEngines[$engType] = [math]::Round(($activeEngines[$engType] + $u), 2)
        }

        if ($engType -match '^(3D|Compute|Graphics)') {
            $util3D += $u
        } elseif ($engType -match '(Video|Codec|Decode|Encode|VPE|JPEG)') {
            $utilVideo += $u
        } elseif ($engType -match 'Copy') {
            $utilCopy += $u
        }
    }

    if ($matchedCount -eq 0) {
        return $emptyResult
    }

    $roundedPerPid = @{}
    foreach ($k in $perPid.Keys) {
        $roundedPerPid[$k] = [math]::Round($perPid[$k], 2)
    }

    return [PSCustomObject]@{
        Available          = $true
        TotalPercent       = [math]::Round($totalUtil, 2)
        MaxEnginePercent   = [math]::Round($maxUtil, 2)
        Engine3DPercent    = [math]::Round($util3D, 2)
        VideoCodecPercent  = [math]::Round($utilVideo, 2)
        CopyPercent        = [math]::Round($utilCopy, 2)
        ActiveEngines      = [PSCustomObject]$activeEngines
        PerPidPercent      = $roundedPerPid
        MatchedSampleCount = $matchedCount
    }
}

function Get-DescendantProcessIds {
    param(
        [Parameter(Mandatory = $true)]
        [int[]]$RootPids
    )
    $allProcs = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name -ErrorAction SilentlyContinue)
    if ($allProcs.Count -eq 0) {
        return @()
    }
    $descendantPids = @(Get-ProcessTreePids -RootPids $RootPids -ProcessTable $allProcs -DescendantsOnly)
    if ($descendantPids.Count -eq 0) {
        return @()
    }
    $descSet = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($dPid in $descendantPids) {
        [void]$descSet.Add($dPid)
    }
    return @($allProcs | Where-Object { $descSet.Contains([int]$_.ProcessId) })
}

# Allow unit tests and wrapper scripts to dot-source pure helpers without executing a live capture.
if ($MyInvocation.InvocationName -eq '.') {
    return
}

$includeTree = $IncludeChildren
if ($PSBoundParameters.ContainsKey('IncludeChildProcesses')) {
    $includeTree = [bool]$IncludeChildProcesses
}

if ($PromptScenario) {
    Write-Host "Select the current process scenario before starting collection:" -ForegroundColor Cyan
    Write-Host "  [1] ocioso      (window open, no video loaded)"
    Write-Host "  [2] pausado     (video loaded and paused)"
    Write-Host "  [3] reproducao  (active video playback)"
    Write-Host "  [4] pesquisa    (search or metadata resolution in progress)"
    Write-Host "  [5] nao-informado / outro"
    $choice = Read-Host "Option (1-5)"
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
            throw "Process with PID $id is not currently running."
        }
        $targetProcesses += $p
    }
} else {
    $found = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Sort-Object Id)
    if ($found.Count -eq 0) {
        throw "No running process named '$ProcessName' was found."
    }
    if ($found.Count -gt 1 -and -not $AggregateMatchingProcesses) {
        $pidList = ($found | ForEach-Object { "$($_.Id) ($($_.ProcessName))" }) -join ', '
        throw "Multiple processes found for '$ProcessName': $pidList. Pass -ProcessId <PID> or -AggregateMatchingProcesses to aggregate all."
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
    Write-Host "Starting non-intrusive process measurement..." -ForegroundColor Cyan
    Write-Host ("  Target     : {0} (PID: {1})" -f $primaryProc.ProcessName, ($targetPids -join ', '))
    if ($executablePath) {
        Write-Host ("  Executable : {0}" -f $executablePath)
    }
    Write-Host ("  Scenario   : {0}" -f $effectiveScenario)
    Write-Host ("  Samples    : {0} (interval: {1}s, child-tree: {2})" -f $SampleCount, $IntervalSeconds, $includeTree)
}

# Baseline CPU times for root targets and any already-running descendants
$baselineTimestamp = Get-Date
$prevCpuMap = @{}
$prevTreeCpuSeconds = @{}
foreach ($p in $targetProcesses) {
    $p.Refresh()
    $pIdInt = [int]$p.Id
    $totSec = $p.TotalProcessorTime.TotalSeconds
    $prevCpuMap[$pIdInt] = [PSCustomObject]@{
        TotalSec  = $totSec
        UserSec   = $p.UserProcessorTime.TotalSeconds
        KernelSec = $p.PrivilegedProcessorTime.TotalSeconds
    }
    $prevTreeCpuSeconds[$pIdInt] = $totSec
}

if ($includeTree) {
    $initialDescendants = @(Get-DescendantProcessIds -RootPids $targetPids)
    foreach ($desc in $initialDescendants) {
        $dPid = [int]$desc.ProcessId
        $dp = Get-Process -Id $dPid -ErrorAction SilentlyContinue
        if ($null -ne $dp) {
            try {
                $prevTreeCpuSeconds[$dPid] = $dp.TotalProcessorTime.TotalSeconds
            } catch {
                # Process may have exited or be protected; skip baseline for this child.
            }
        }
    }
}

$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$prevElapsedSec = $stopwatch.Elapsed.TotalSeconds
$lastIterationEndSec = $prevElapsedSec

$samples = [System.Collections.Generic.List[object]]::new()
$anyGpuCounterSeen = $false
$anyTreeGpuCounterSeen = $false

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
        Write-Warning "Monitored process exited before sample $i."
        break
    }

    $aliveRootPids = @($aliveTargets | ForEach-Object { [int]$_.Id })
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

    # Collect alive descendant processes when tree aggregation is enabled.
    $aliveChildren = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
    if ($includeTree) {
        $descendants = @(Get-DescendantProcessIds -RootPids $aliveRootPids)
        foreach ($desc in $descendants) {
            $cPid = [int]$desc.ProcessId
            $cp = Get-Process -Id $cPid -ErrorAction SilentlyContinue
            if ($null -ne $cp) {
                $cp.Refresh()
                $aliveChildren.Add($cp)
            }
        }
    }

    $allTreePids = @($aliveRootPids)
    foreach ($cp in $aliveChildren) {
        $allTreePids += [int]$cp.Id
    }

    # Fetch WorkingSetPrivate for all tree PIDs in a single CIM query.
    $wspByPid = @{}
    $allPidFilter = ($allTreePids | ForEach-Object { "IDProcess = $_" }) -join ' OR '
    $perfProcItems = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter $allPidFilter -ErrorAction SilentlyContinue)
    foreach ($item in $perfProcItems) {
        if ($null -ne $item.IDProcess -and $null -ne $item.WorkingSetPrivate) {
            $itemPid = [int]$item.IDProcess
            if (-not $wspByPid.ContainsKey($itemPid)) {
                $wspByPid[$itemPid] = [int64]$item.WorkingSetPrivate
            }
        }
    }

    $treeSnapshots = [System.Collections.Generic.List[object]]::new()
    foreach ($p in $aliveTargets) {
        $pidInt = [int]$p.Id
        $wspVal = if ($wspByPid.ContainsKey($pidInt)) { $wspByPid[$pidInt] } else { $null }
        $treeSnapshots.Add([PSCustomObject]@{
            ProcessId                 = $pidInt
            ProcessName               = $p.ProcessName
            WorkingSetBytes           = [int64]$p.WorkingSet64
            PrivateBytes              = [int64]$p.PrivateMemorySize64
            WorkingSetPrivateBytes    = $wspVal
            TotalProcessorTimeSeconds = $p.TotalProcessorTime.TotalSeconds
            IsNewChild                = $false
        })
    }
    foreach ($cp in $aliveChildren) {
        $cPidInt = [int]$cp.Id
        $cWspVal = if ($wspByPid.ContainsKey($cPidInt)) { $wspByPid[$cPidInt] } else { $null }
        $cCpuSec = 0.0
        $isNewChild = $false
        try {
            $cCpuSec = $cp.TotalProcessorTime.TotalSeconds
        } catch {
            $cCpuSec = 0.0
        }
        try {
            if ($null -ne $cp.StartTime -and $cp.StartTime -ge $baselineTimestamp) {
                $isNewChild = $true
            }
        } catch {
            $isNewChild = $false
        }
        $treeSnapshots.Add([PSCustomObject]@{
            ProcessId                 = $cPidInt
            ProcessName               = $cp.ProcessName
            WorkingSetBytes           = [int64]$cp.WorkingSet64
            PrivateBytes              = [int64]$cp.PrivateMemorySize64
            WorkingSetPrivateBytes    = $cWspVal
            TotalProcessorTimeSeconds = $cCpuSec
            IsNewChild                = $isNewChild
        })
    }

    $treeAgg = Aggregate-ProcessTreeSample `
        -RootPids $aliveRootPids `
        -ProcessSnapshots @($treeSnapshots.ToArray()) `
        -PreviousCpuSeconds $prevTreeCpuSeconds `
        -DeltaWallSeconds $deltaWallSec `
        -LogicalProcessors $logicalProcessors
    $prevTreeCpuSeconds = $treeAgg.NextCpuSeconds

    $workingSetPrivateBytes = $treeAgg.RootWorkingSetPrivateBytes

    # Fetch GPU counters across the full process tree in a single WMI pass per counter class.
    $treeGpuFilter = ($allTreePids | ForEach-Object { "Name LIKE 'pid_${_}_%'" }) -join ' OR '
    $treeGpuPidPattern = '^pid_(' + (($allTreePids | ForEach-Object { [string]$_ }) -join '|') + ')_'
    $rootGpuPidPattern = '^pid_(' + (($aliveRootPids | ForEach-Object { [string]$_ }) -join '|') + ')_'

    $allGpuEngines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -Filter $treeGpuFilter -ErrorAction SilentlyContinue | Where-Object { $_.Name -match $treeGpuPidPattern })
    $allGpuMemItems = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -Filter $treeGpuFilter -ErrorAction SilentlyContinue | Where-Object { $_.Name -match $treeGpuPidPattern })

    $rootGpuParsed = Parse-GpuEngineUtilization -CounterSamples $allGpuEngines -TargetPids $aliveRootPids
    $treeGpuParsed = Parse-GpuEngineUtilization -CounterSamples $allGpuEngines -TargetPids $allTreePids

    $rootGpuMemItems = @($allGpuMemItems | Where-Object { $_.Name -match $rootGpuPidPattern })
    $gpuSampleAvailable = ($rootGpuParsed.Available -or $rootGpuMemItems.Count -gt 0)
    $treeGpuSampleAvailable = ($treeGpuParsed.Available -or $allGpuMemItems.Count -gt 0)

    if ($gpuSampleAvailable) {
        $anyGpuCounterSeen = $true
    }
    if ($treeGpuSampleAvailable) {
        $anyTreeGpuCounterSeen = $true
    }

    $gpuDedicatedBytes = $null
    $gpuSharedBytes = $null
    $gpuLocalBytes = $null
    if ($rootGpuMemItems.Count -gt 0) {
        $dedSum = [int64]0
        $shrSum = [int64]0
        $locSum = [int64]0
        foreach ($m in $rootGpuMemItems) {
            if ($null -ne $m.DedicatedUsage) { $dedSum += [int64]$m.DedicatedUsage }
            if ($null -ne $m.SharedUsage) { $shrSum += [int64]$m.SharedUsage }
            if ($null -ne $m.LocalUsage) { $locSum += [int64]$m.LocalUsage }
        }
        $gpuDedicatedBytes = $dedSum
        $gpuSharedBytes = $shrSum
        $gpuLocalBytes = $locSum
    }

    $treeGpuDedicatedBytes = $null
    $treeGpuSharedBytes = $null
    if ($allGpuMemItems.Count -gt 0) {
        $tDedSum = [int64]0
        $tShrSum = [int64]0
        foreach ($m in $allGpuMemItems) {
            if ($null -ne $m.DedicatedUsage) { $tDedSum += [int64]$m.DedicatedUsage }
            if ($null -ne $m.SharedUsage) { $tShrSum += [int64]$m.SharedUsage }
        }
        $treeGpuDedicatedBytes = $tDedSum
        $treeGpuSharedBytes = $tShrSum
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
        GpuEngineTotalPercent      = $rootGpuParsed.TotalPercent
        GpuEngineMaxPercent        = $rootGpuParsed.MaxEnginePercent
        Gpu3DPercent               = $rootGpuParsed.Engine3DPercent
        GpuVideoCodecPercent       = $rootGpuParsed.VideoCodecPercent
        GpuCopyPercent             = $rootGpuParsed.CopyPercent
        GpuActiveEngines           = $rootGpuParsed.ActiveEngines
        GpuDedicatedBytes          = $gpuDedicatedBytes
        GpuDedicatedMiB            = ConvertTo-MiB $gpuDedicatedBytes
        GpuSharedBytes             = $gpuSharedBytes
        GpuSharedMiB               = ConvertTo-MiB $gpuSharedBytes
        GpuLocalBytes              = $gpuLocalBytes
        GpuLocalMiB                = ConvertTo-MiB $gpuLocalBytes
        ChildProcessCount          = $treeAgg.ChildProcessCount
        ChildProcessNames          = ($treeAgg.ChildProcessNames -join ', ')
        ChildCpuPercent            = $treeAgg.ChildCpuPercent
        ChildWorkingSetPrivateMiB  = ConvertTo-MiB $treeAgg.ChildWorkingSetPrivateBytes
        ChildWorkingSetMiB         = ConvertTo-MiB $treeAgg.ChildWorkingSetBytes
        ChildPrivateMiB            = ConvertTo-MiB $treeAgg.ChildPrivateBytes
        TreeProcessCount           = $treeAgg.TreeProcessCount
        TreeCpuPercent             = $treeAgg.TreeCpuPercent
        TreeCpuSingleCorePercent   = $treeAgg.TreeCpuSingleCorePercent
        TreeWorkingSetMB           = $treeAgg.TreeWorkingSetMB
        TreePrivateMB              = $treeAgg.TreePrivateMB
        TreeWorkingSetPrivateMB    = $treeAgg.TreeWorkingSetPrivateMB
        TreeGpuAvailable           = $treeGpuSampleAvailable
        TreeGpuPercent             = $treeGpuParsed.TotalPercent
        TreeGpu3DPercent           = $treeGpuParsed.Engine3DPercent
        TreeGpuVideoCodecPercent   = $treeGpuParsed.VideoCodecPercent
        TreeGpuDedicatedMiB        = ConvertTo-MiB $treeGpuDedicatedBytes
        TreeGpuSharedMiB           = ConvertTo-MiB $treeGpuSharedBytes
    }

    $samples.Add($sampleObj)
    $lastIterationEndSec = $stopwatch.Elapsed.TotalSeconds
}

if ($samples.Count -eq 0) {
    throw "No samples could be collected."
}

$summary = [PSCustomObject]@{
    WorkingSetPrivateMiB    = Get-MetricStats -Values @($samples | ForEach-Object { $_.WorkingSetPrivateMiB })
    WorkingSetMiB           = Get-MetricStats -Values @($samples | ForEach-Object { $_.WorkingSetMiB })
    PeakWorkingSetMiB       = Get-MetricStats -Values @($samples | ForEach-Object { $_.PeakWorkingSetMiB })
    PrivateMiB              = Get-MetricStats -Values @($samples | ForEach-Object { $_.PrivateMiB })
    CpuTotalPercent         = Get-MetricStats -Values @($samples | ForEach-Object { $_.CpuTotalPercent })
    CpuSingleCorePercent    = Get-MetricStats -Values @($samples | ForEach-Object { $_.CpuSingleCorePercent })
    ThreadCount             = Get-MetricStats -Values @($samples | ForEach-Object { [double]$_.ThreadCount })
    HandleCount             = Get-MetricStats -Values @($samples | ForEach-Object { [double]$_.HandleCount })
    GpuAvailable            = $anyGpuCounterSeen
    GpuEngineTotalPercent   = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuEngineTotalPercent }) } else { $null }
    Gpu3DPercent            = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.Gpu3DPercent }) } else { $null }
    GpuVideoCodecPercent    = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuVideoCodecPercent }) } else { $null }
    GpuDedicatedMiB         = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuDedicatedMiB }) } else { $null }
    GpuSharedMiB            = if ($anyGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.GpuSharedMiB }) } else { $null }
    MaxChildProcessCount    = ($samples | Measure-Object -Property ChildProcessCount -Maximum).Maximum
    ChildProcessCount       = Get-MetricStats -Values @($samples | ForEach-Object { [double]$_.ChildProcessCount })
    TreeProcessCount        = Get-MetricStats -Values @($samples | ForEach-Object { [double]$_.TreeProcessCount })
    TreeCpuPercent          = Get-MetricStats -Values @($samples | ForEach-Object { $_.TreeCpuPercent })
    TreeWorkingSetMB        = Get-MetricStats -Values @($samples | ForEach-Object { $_.TreeWorkingSetMB })
    TreePrivateMB           = Get-MetricStats -Values @($samples | ForEach-Object { $_.TreePrivateMB })
    TreeWorkingSetPrivateMB = Get-MetricStats -Values @($samples | ForEach-Object { $_.TreeWorkingSetPrivateMB })
    TreeGpuAvailable        = $anyTreeGpuCounterSeen
    TreeGpuPercent          = if ($anyTreeGpuCounterSeen) { Get-MetricStats -Values @($samples | ForEach-Object { $_.TreeGpuPercent }) } else { $null }
}

$report = [PSCustomObject]@{
    SchemaVersion  = '1.0'
    CollectedAt    = (Get-Date).ToString('o')
    Scenario       = $effectiveScenario
    ScenarioSource = 'manual'
    VideoUrl       = $VideoUrl
    Resolution     = $Resolution
    Notes          = $Notes
    Target         = [PSCustomObject]@{
        ProcessName        = $primaryProc.ProcessName
        ProcessIds         = $targetPids
        ExecutablePath     = $executablePath
        ProcessStartTime   = $startTimeIso
        AggregatedMatching = [bool]$AggregateMatchingProcesses
        IncludeChildren    = [bool]$includeTree
    }
    Sampling       = [PSCustomObject]@{
        RequestedSamples = $SampleCount
        CollectedSamples = $samples.Count
        IntervalSeconds  = $IntervalSeconds
        TotalDurationSec = [math]::Round($stopwatch.Elapsed.TotalSeconds, 2)
    }
    Environment    = $environmentMetadata
    Summary        = $summary
    Samples        = @($samples)
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
        "| Tree Working Set (`TreeWorkingSetMB`, MiB) | $($summary.TreeWorkingSetMB.Min) | $($summary.TreeWorkingSetMB.Avg) | $($summary.TreeWorkingSetMB.Median) | $($summary.TreeWorkingSetMB.P95) | $($summary.TreeWorkingSetMB.Max) |"
        "| Tree Private Memory (`TreePrivateMB`, MiB) | $($summary.TreePrivateMB.Min) | $($summary.TreePrivateMB.Avg) | $($summary.TreePrivateMB.Median) | $($summary.TreePrivateMB.P95) | $($summary.TreePrivateMB.Max) |"
        "| CPU Total da Maquina (%) | $($summary.CpuTotalPercent.Min) | $($summary.CpuTotalPercent.Avg) | $($summary.CpuTotalPercent.Median) | $($summary.CpuTotalPercent.P95) | $($summary.CpuTotalPercent.Max) |"
        "| Tree CPU Total (%) | $($summary.TreeCpuPercent.Min) | $($summary.TreeCpuPercent.Avg) | $($summary.TreeCpuPercent.Median) | $($summary.TreeCpuPercent.P95) | $($summary.TreeCpuPercent.Max) |"
        "| CPU Equivalente 1 Nucleo (%) | $($summary.CpuSingleCorePercent.Min) | $($summary.CpuSingleCorePercent.Avg) | $($summary.CpuSingleCorePercent.Median) | $($summary.CpuSingleCorePercent.P95) | $($summary.CpuSingleCorePercent.Max) |"
        "| GPU Total Engines (%) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuEngineTotalPercent.Max } else { 'N/D' }) |"
        "| Tree GPU Total (%) | $(if ($summary.TreeGpuAvailable) { $summary.TreeGpuPercent.Min } else { 'N/D' }) | $(if ($summary.TreeGpuAvailable) { $summary.TreeGpuPercent.Avg } else { 'N/D' }) | $(if ($summary.TreeGpuAvailable) { $summary.TreeGpuPercent.Median } else { 'N/D' }) | $(if ($summary.TreeGpuAvailable) { $summary.TreeGpuPercent.P95 } else { 'N/D' }) | $(if ($summary.TreeGpuAvailable) { $summary.TreeGpuPercent.Max } else { 'N/D' }) |"
        "| GPU Video Codec/Decode (%) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuVideoCodecPercent.Max } else { 'N/D' }) |"
        "| GPU 3D/Compute (%) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.Gpu3DPercent.Max } else { 'N/D' }) |"
        "| GPU VRAM Dedicada (MiB) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuDedicatedMiB.Max } else { 'N/D' }) |"
        "| GPU VRAM Compartilhada (MiB) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Min } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Avg } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Median } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.P95 } else { 'N/D' }) | $(if ($summary.GpuAvailable) { $summary.GpuSharedMiB.Max } else { 'N/D' }) |"
        "| Child Processes (count) | $($summary.ChildProcessCount.Min) | $($summary.ChildProcessCount.Avg) | $($summary.ChildProcessCount.Median) | $($summary.ChildProcessCount.P95) | $($summary.ChildProcessCount.Max) |"
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
        Write-Host "Collection Summary ($($report.Sampling.CollectedSamples) samples, $($report.Sampling.TotalDurationSec)s):" -ForegroundColor Green
        Write-Host ("  WorkingSetPrivate (Root)          : avg {0} MiB (min {1} / max {2} MiB)" -f $summary.WorkingSetPrivateMiB.Avg, $summary.WorkingSetPrivateMiB.Min, $summary.WorkingSetPrivateMiB.Max)
        Write-Host ("  WorkingSet64 (Root / Tree)        : root avg {0} MiB / tree avg {1} MiB" -f $summary.WorkingSetMiB.Avg, $summary.TreeWorkingSetMB.Avg)
        Write-Host ("  PrivateBytes (Root / Tree)        : root avg {0} MiB / tree avg {1} MiB" -f $summary.PrivateMiB.Avg, $summary.TreePrivateMB.Avg)
        Write-Host ("  CPU Total ({0} logical cores)      : root avg {1}% / tree avg {2}%" -f $logicalProcessors, $summary.CpuTotalPercent.Avg, $summary.TreeCpuPercent.Avg)
        Write-Host ("  Child Processes                   : avg {0} (max {1})" -f $summary.ChildProcessCount.Avg, $summary.MaxChildProcessCount)
        if ($summary.TreeGpuAvailable) {
            Write-Host ("  GPU Tree Utilization              : avg {0}% (max {1}%)" -f $summary.TreeGpuPercent.Avg, $summary.TreeGpuPercent.Max)
        } else {
            Write-Host "  GPU                               : WDDM counters unavailable for target process tree"
        }
    }
    return $report
}
