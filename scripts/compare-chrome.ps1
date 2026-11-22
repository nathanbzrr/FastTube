<#
.SYNOPSIS
    Multi-process comparative benchmark runner: FastTube vs. Chrome / Edge.

.DESCRIPTION
    Executes `scripts/measure-process.ps1` with full child-process-tree aggregation against
    both FastTube (`fasttube.exe`) and a Chromium browser (`chrome.exe` or `msedge.exe`) across
    one or more configurable phases, computes exact resource deltas and reduction ratios
    (RAM MB, CPU %, GPU %, process count), and outputs both a JSON report and a formatted
    Markdown comparison table.

    Supports offline testing via dot-sourcing (`. .\scripts\compare-chrome.ps1`) or passing
    a pre-recorded fixture file via `-DryRunFixture`.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [string]$FastTubeProcessName = 'fasttube',

    [Parameter(Mandatory = $false)]
    [int[]]$FastTubeProcessId,

    [Parameter(Mandatory = $false)]
    [ValidateSet('auto', 'chrome', 'msedge')]
    [string]$BrowserProcessName = 'auto',

    [Parameter(Mandatory = $false)]
    [int[]]$BrowserProcessId,

    [Parameter(Mandatory = $false)]
    [string]$VideoUrl = '',

    [Parameter(Mandatory = $false)]
    [string]$Resolution = '1080p',

    [Parameter(Mandatory = $false)]
    [string[]]$Phases = @('playback'),

    [Parameter(Mandatory = $false)]
    [ValidateRange(1, 3600)]
    [int]$DurationSeconds = 15,

    [Parameter(Mandatory = $false)]
    [ValidateRange(0.2, 60.0)]
    [double]$IntervalSeconds = 1.0,

    [Parameter(Mandatory = $false)]
    [string]$Notes = '',

    [Parameter(Mandatory = $false)]
    [string]$OutputJson = '',

    [Parameter(Mandatory = $false)]
    [string]$OutputMarkdown = '',

    [Parameter(Mandatory = $false)]
    [string]$DryRunFixture = '',

    [Parameter(Mandatory = $false)]
    [switch]$AsJson,

    [Parameter(Mandatory = $false)]
    [switch]$Quiet
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-NestedProperty {
    param(
        [Parameter(Mandatory = $false)]$Object,
        [Parameter(Mandatory = $true)][string[]]$Path,
        [Parameter(Mandatory = $false)]$DefaultValue = $null
    )
    $current = $Object
    foreach ($segment in $Path) {
        if ($null -eq $current) { return $DefaultValue }
        if ($current -is [System.Collections.IDictionary]) {
            if (-not $current.Contains($segment)) { return $DefaultValue }
            $current = $current[$segment]
        } else {
            $prop = $current.PSObject.Properties[$segment]
            if ($null -eq $prop) { return $DefaultValue }
            $current = $prop.Value
        }
    }
    return $current
}

function Get-ComparisonMetricDelta {
    <#
    .SYNOPSIS
        Computes exact delta, savings, reduction percentage, and ratio between FastTube and Browser values.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $false)][AllowNull()]$FastTubeValue,
        [Parameter(Mandatory = $false)][AllowNull()]$BrowserValue
    )

    if ($null -eq $FastTubeValue -or $null -eq $BrowserValue) {
        return [PSCustomObject]@{
            FastTube         = if ($null -ne $FastTubeValue) { [math]::Round([double]$FastTubeValue, 2) } else { $null }
            Browser          = if ($null -ne $BrowserValue) { [math]::Round([double]$BrowserValue, 2) } else { $null }
            Delta            = $null
            Savings          = $null
            ReductionPercent = $null
            Factor           = $null
        }
    }

    $ft = [double]$FastTubeValue
    $br = [double]$BrowserValue
    $delta = [math]::Round(($ft - $br), 2)
    $savings = [math]::Round(($br - $ft), 2)
    $reductionPct = if ($br -gt 0.0) {
        [math]::Round((($br - $ft) / $br) * 100.0, 1)
    } else {
        $null
    }
    $factor = if ($ft -gt 0.0 -and $br -gt 0.0) {
        [math]::Round(($br / $ft), 2)
    } else {
        $null
    }

    return [PSCustomObject]@{
        FastTube         = [math]::Round($ft, 2)
        Browser          = [math]::Round($br, 2)
        Delta            = $delta
        Savings          = $savings
        ReductionPercent = $reductionPct
        Factor           = $factor
    }
}

function Resolve-ReportTreeStat {
    param(
        [Parameter(Mandatory = $true)]$Summary,
        [Parameter(Mandatory = $true)][string]$PrimaryMetric,
        [Parameter(Mandatory = $true)][string]$FallbackMetric,
        [Parameter(Mandatory = $true)][string]$StatField
    )
    $primary = Get-NestedProperty -Object $Summary -Path @($PrimaryMetric, $StatField)
    if ($null -ne $primary) { return $primary }
    return Get-NestedProperty -Object $Summary -Path @($FallbackMetric, $StatField)
}

function Compare-ProcessReports {
    <#
    .SYNOPSIS
        Compares a FastTube measurement report and a Browser measurement report from measure-process.ps1.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$FastTubeReport,
        [Parameter(Mandatory = $true)]$BrowserReport,
        [Parameter(Mandatory = $false)][string]$Phase = 'playback'
    )

    $ftSummary = Get-NestedProperty -Object $FastTubeReport -Path @('Summary')
    $brSummary = Get-NestedProperty -Object $BrowserReport -Path @('Summary')
    if ($null -eq $ftSummary -or $null -eq $brSummary) {
        throw 'Both FastTubeReport and BrowserReport must include a Summary object.'
    }

    $ftWsAvg = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeWorkingSetMB' -FallbackMetric 'WorkingSetMiB' -StatField 'Avg'
    $ftWsP95 = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeWorkingSetMB' -FallbackMetric 'WorkingSetMiB' -StatField 'P95'
    $brWsAvg = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeWorkingSetMB' -FallbackMetric 'WorkingSetMiB' -StatField 'Avg'
    $brWsP95 = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeWorkingSetMB' -FallbackMetric 'WorkingSetMiB' -StatField 'P95'

    $ftPrivAvg = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreePrivateMB' -FallbackMetric 'PrivateMiB' -StatField 'Avg'
    $ftPrivP95 = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreePrivateMB' -FallbackMetric 'PrivateMiB' -StatField 'P95'
    $brPrivAvg = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreePrivateMB' -FallbackMetric 'PrivateMiB' -StatField 'Avg'
    $brPrivP95 = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreePrivateMB' -FallbackMetric 'PrivateMiB' -StatField 'P95'

    $ftWspAvg = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeWorkingSetPrivateMB' -FallbackMetric 'WorkingSetPrivateMiB' -StatField 'Avg'
    $ftWspP95 = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeWorkingSetPrivateMB' -FallbackMetric 'WorkingSetPrivateMiB' -StatField 'P95'
    $brWspAvg = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeWorkingSetPrivateMB' -FallbackMetric 'WorkingSetPrivateMiB' -StatField 'Avg'
    $brWspP95 = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeWorkingSetPrivateMB' -FallbackMetric 'WorkingSetPrivateMiB' -StatField 'P95'

    $ftCpuAvg = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeCpuPercent' -FallbackMetric 'CpuTotalPercent' -StatField 'Avg'
    $ftCpuP95 = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeCpuPercent' -FallbackMetric 'CpuTotalPercent' -StatField 'P95'
    $brCpuAvg = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeCpuPercent' -FallbackMetric 'CpuTotalPercent' -StatField 'Avg'
    $brCpuP95 = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeCpuPercent' -FallbackMetric 'CpuTotalPercent' -StatField 'P95'

    $ftGpuAvg = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeGpuPercent' -FallbackMetric 'GpuEngineTotalPercent' -StatField 'Avg'
    $ftGpuP95 = Resolve-ReportTreeStat -Summary $ftSummary -PrimaryMetric 'TreeGpuPercent' -FallbackMetric 'GpuEngineTotalPercent' -StatField 'P95'
    $brGpuAvg = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeGpuPercent' -FallbackMetric 'GpuEngineTotalPercent' -StatField 'Avg'
    $brGpuP95 = Resolve-ReportTreeStat -Summary $brSummary -PrimaryMetric 'TreeGpuPercent' -FallbackMetric 'GpuEngineTotalPercent' -StatField 'P95'

    $ftProcAvg = Get-NestedProperty -Object $ftSummary -Path @('TreeProcessCount', 'Avg')
    if ($null -eq $ftProcAvg) {
        $ftChildMax = Get-NestedProperty -Object $ftSummary -Path @('MaxChildProcessCount') -DefaultValue 0
        $ftProcAvg = 1 + [double]$ftChildMax
    }
    $ftProcMax = Get-NestedProperty -Object $ftSummary -Path @('TreeProcessCount', 'Max')
    if ($null -eq $ftProcMax) { $ftProcMax = $ftProcAvg }

    $brProcAvg = Get-NestedProperty -Object $brSummary -Path @('TreeProcessCount', 'Avg')
    if ($null -eq $brProcAvg) {
        $brChildMax = Get-NestedProperty -Object $brSummary -Path @('MaxChildProcessCount') -DefaultValue 0
        $brProcAvg = 1 + [double]$brChildMax
    }
    $brProcMax = Get-NestedProperty -Object $brSummary -Path @('TreeProcessCount', 'Max')
    if ($null -eq $brProcMax) { $brProcMax = $brProcAvg }

    $ftName = Get-NestedProperty -Object $FastTubeReport -Path @('Target', 'ProcessName') -DefaultValue 'fasttube'
    $brName = Get-NestedProperty -Object $BrowserReport -Path @('Target', 'ProcessName') -DefaultValue 'chrome'

    return [PSCustomObject]@{
        Phase          = $Phase
        FastTubeTarget = [string]$ftName
        BrowserTarget  = [string]$brName
        Metrics        = [PSCustomObject]@{
            WorkingSetMB        = [PSCustomObject]@{
                Avg = Get-ComparisonMetricDelta -FastTubeValue $ftWsAvg -BrowserValue $brWsAvg
                P95 = Get-ComparisonMetricDelta -FastTubeValue $ftWsP95 -BrowserValue $brWsP95
            }
            PrivateMB           = [PSCustomObject]@{
                Avg = Get-ComparisonMetricDelta -FastTubeValue $ftPrivAvg -BrowserValue $brPrivAvg
                P95 = Get-ComparisonMetricDelta -FastTubeValue $ftPrivP95 -BrowserValue $brPrivP95
            }
            WorkingSetPrivateMB = [PSCustomObject]@{
                Avg = Get-ComparisonMetricDelta -FastTubeValue $ftWspAvg -BrowserValue $brWspAvg
                P95 = Get-ComparisonMetricDelta -FastTubeValue $ftWspP95 -BrowserValue $brWspP95
            }
            CpuPercent          = [PSCustomObject]@{
                Avg = Get-ComparisonMetricDelta -FastTubeValue $ftCpuAvg -BrowserValue $brCpuAvg
                P95 = Get-ComparisonMetricDelta -FastTubeValue $ftCpuP95 -BrowserValue $brCpuP95
            }
            GpuPercent          = [PSCustomObject]@{
                Avg = Get-ComparisonMetricDelta -FastTubeValue $ftGpuAvg -BrowserValue $brGpuAvg
                P95 = Get-ComparisonMetricDelta -FastTubeValue $ftGpuP95 -BrowserValue $brGpuP95
            }
            ProcessCount        = [PSCustomObject]@{
                Avg = Get-ComparisonMetricDelta -FastTubeValue $ftProcAvg -BrowserValue $brProcAvg
                Max = Get-ComparisonMetricDelta -FastTubeValue $ftProcMax -BrowserValue $brProcMax
            }
        }
    }
}

function Format-MetricPairCell {
    param(
        [Parameter(Mandatory = $false)][AllowNull()]$Primary,
        [Parameter(Mandatory = $false)][AllowNull()]$Secondary,
        [Parameter(Mandatory = $false)][string]$Unit = ''
    )
    if ($null -eq $Primary -and $null -eq $Secondary) { return 'N/A' }
    $pText = if ($null -ne $Primary) { "{0}{1}" -f $Primary, $Unit } else { 'N/A' }
    $sText = if ($null -ne $Secondary) { "{0}{1}" -f $Secondary, $Unit } else { 'N/A' }
    return "$pText / $sText"
}

function Format-SignedDeltaCell {
    param(
        [Parameter(Mandatory = $false)][AllowNull()]$Delta,
        [Parameter(Mandatory = $false)][string]$Unit = ''
    )
    if ($null -eq $Delta) { return 'N/A' }
    $d = [double]$Delta
    $sign = if ($d -gt 0) { '+' } else { '' }
    return "{0}{1}{2}" -f $sign, $d, $Unit
}

function Format-ChromeComparisonSummary {
    <#
    .SYNOPSIS
        Formats one or more FastTube-vs-Browser comparison objects into a Markdown table.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Comparison
    )

    $phaseList = @()
    $phasesProp = Get-NestedProperty -Object $Comparison -Path @('Phases')
    if ($null -ne $phasesProp) {
        $phaseList = @($phasesProp)
    } else {
        $phaseList = @($Comparison)
    }

    $generatedAt = Get-NestedProperty -Object $Comparison -Path @('GeneratedAt') -DefaultValue ((Get-Date).ToString('o'))
    $videoUrl = Get-NestedProperty -Object $Comparison -Path @('VideoUrl') -DefaultValue ''
    $resolution = Get-NestedProperty -Object $Comparison -Path @('Resolution') -DefaultValue ''
    $durationSec = Get-NestedProperty -Object $Comparison -Path @('DurationSeconds')
    $notes = Get-NestedProperty -Object $Comparison -Path @('Notes') -DefaultValue ''

    $lines = [System.Collections.Generic.List[string]]::new()
    $lines.Add("## FastTube vs. Browser Comparative Benchmark")
    $lines.Add("")
    $lines.Add("- **Generated**: ``$generatedAt``")
    if (-not [string]::IsNullOrWhiteSpace($videoUrl)) {
        $lines.Add("- **Video URL**: ``$videoUrl``")
    }
    if (-not [string]::IsNullOrWhiteSpace($resolution)) {
        $lines.Add("- **Resolution**: ``$resolution``")
    }
    if ($null -ne $durationSec) {
        $lines.Add("- **Window**: ``${durationSec}s`` per target/phase (full process tree)")
    }
    if (-not [string]::IsNullOrWhiteSpace($notes)) {
        $lines.Add("- **Notes**: $notes")
    }

    foreach ($phaseItem in $phaseList) {
        $phaseName = Get-NestedProperty -Object $phaseItem -Path @('Phase') -DefaultValue 'playback'
        $ftLabel = Get-NestedProperty -Object $phaseItem -Path @('FastTubeTarget') -DefaultValue 'fasttube'
        $brLabel = Get-NestedProperty -Object $phaseItem -Path @('BrowserTarget') -DefaultValue 'chrome'
        $m = Get-NestedProperty -Object $phaseItem -Path @('Metrics')

        $lines.Add("")
        $lines.Add("### Phase: ``$phaseName`` (``$ftLabel`` vs ``$brLabel``)")
        $lines.Add("")
        $lines.Add("| Metric | FastTube (Avg / P95) | Browser (Avg / P95) | Delta (Avg) | Reduction | Factor |")
        $lines.Add("| :--- | ---: | ---: | ---: | ---: | ---: |")

        $rows = @(
            @{ Label = 'Tree Working Set (MB)'; Metric = $m.WorkingSetMB; Second = 'P95'; Unit = ' MB' }
            @{ Label = 'Tree Private Memory (MB)'; Metric = $m.PrivateMB; Second = 'P95'; Unit = ' MB' }
            @{ Label = 'Tree Working Set Private (MB)'; Metric = $m.WorkingSetPrivateMB; Second = 'P95'; Unit = ' MB' }
            @{ Label = 'Tree CPU Utilization (%)'; Metric = $m.CpuPercent; Second = 'P95'; Unit = '%' }
            @{ Label = 'Tree GPU Utilization (%)'; Metric = $m.GpuPercent; Second = 'P95'; Unit = '%' }
            @{ Label = 'Process Tree Count (Avg / Max)'; Metric = $m.ProcessCount; Second = 'Max'; Unit = '' }
        )

        foreach ($row in $rows) {
            $avgObj = Get-NestedProperty -Object $row.Metric -Path @('Avg')
            $secObj = Get-NestedProperty -Object $row.Metric -Path @($row.Second)

            $ftCell = Format-MetricPairCell -Primary (Get-NestedProperty -Object $avgObj -Path @('FastTube')) -Secondary (Get-NestedProperty -Object $secObj -Path @('FastTube')) -Unit $row.Unit
            $brCell = Format-MetricPairCell -Primary (Get-NestedProperty -Object $avgObj -Path @('Browser')) -Secondary (Get-NestedProperty -Object $secObj -Path @('Browser')) -Unit $row.Unit
            $deltaCell = Format-SignedDeltaCell -Delta (Get-NestedProperty -Object $avgObj -Path @('Delta')) -Unit $row.Unit

            $redVal = Get-NestedProperty -Object $avgObj -Path @('ReductionPercent')
            $redCell = if ($null -ne $redVal) { "{0}%" -f $redVal } else { 'N/A' }

            $facVal = Get-NestedProperty -Object $avgObj -Path @('Factor')
            $facCell = if ($null -ne $facVal) { "{0}x" -f $facVal } else { 'N/A' }

            $lines.Add("| $($row.Label) | $ftCell | $brCell | $deltaCell | $redCell | $facCell |")
        }
    }

    return ($lines -join [Environment]::NewLine)
}

function Map-PhaseToScenario {
    param([Parameter(Mandatory = $true)][string]$Phase)
    switch ($Phase.Trim().ToLowerInvariant()) {
        'idle' { return 'ocioso' }
        'ocioso' { return 'ocioso' }
        'paused' { return 'pausado' }
        'pausado' { return 'pausado' }
        'playback' { return 'reproducao' }
        'reproducao' { return 'reproducao' }
        'search' { return 'pesquisa' }
        'pesquisa' { return 'pesquisa' }
        default { return 'comparacao-navegador' }
    }
}

# Allow unit tests to dot-source pure helpers without executing live benchmarks.
if ($MyInvocation.InvocationName -eq '.') {
    return
}

$phaseResults = [System.Collections.Generic.List[object]]::new()

if (-not [string]::IsNullOrWhiteSpace($DryRunFixture)) {
    if (-not (Test-Path -LiteralPath $DryRunFixture -PathType Leaf)) {
        throw "Dry-run fixture not found: $DryRunFixture"
    }
    $fixture = Get-Content -LiteralPath $DryRunFixture -Raw -Encoding UTF8 | ConvertFrom-Json
    $fixturePhases = Get-NestedProperty -Object $fixture -Path @('Phases')
    if ($null -ne $fixturePhases) {
        foreach ($fp in @($fixturePhases)) {
            $pName = Get-NestedProperty -Object $fp -Path @('Phase') -DefaultValue 'playback'
            $phaseResults.Add((Compare-ProcessReports -FastTubeReport $fp.FastTube -BrowserReport $fp.Browser -Phase $pName))
        }
    } else {
        $pName = if ($Phases.Count -gt 0) { $Phases[0] } else { 'playback' }
        $phaseResults.Add((Compare-ProcessReports -FastTubeReport $fixture.FastTube -BrowserReport $fixture.Browser -Phase $pName))
    }
} else {
    $measureScript = Join-Path $PSScriptRoot 'measure-process.ps1'
    if (-not (Test-Path -LiteralPath $measureScript -PathType Leaf)) {
        throw "Required measurement script not found: $measureScript"
    }

    $resolvedBrowserName = $BrowserProcessName
    if ($null -eq $BrowserProcessId -or $BrowserProcessId.Count -eq 0) {
        if ($BrowserProcessName -eq 'auto') {
            $chromeProcs = @(Get-Process -Name 'chrome' -ErrorAction SilentlyContinue)
            $edgeProcs = @(Get-Process -Name 'msedge' -ErrorAction SilentlyContinue)
            if ($chromeProcs.Count -gt 0) {
                $resolvedBrowserName = 'chrome'
            } elseif ($edgeProcs.Count -gt 0) {
                $resolvedBrowserName = 'msedge'
            } else {
                throw "Neither 'chrome' nor 'msedge' is currently running. Start a browser or pass -BrowserProcessId."
            }
        }
    }

    $sampleCount = [math]::Max(1, [int][math]::Ceiling($DurationSeconds / $IntervalSeconds))

    foreach ($phase in $Phases) {
        $mappedScenario = Map-PhaseToScenario -Phase $phase

        if (-not $Quiet -and -not $AsJson) {
            Write-Host "Running comparative phase '$phase' ($sampleCount samples @ ${IntervalSeconds}s)..." -ForegroundColor Cyan
        }

        $ftParams = @{
            Scenario              = $mappedScenario
            CustomScenarioLabel   = $phase
            SampleCount           = $sampleCount
            IntervalSeconds       = $IntervalSeconds
            IncludeChildProcesses = $true
            VideoUrl              = $VideoUrl
            Resolution            = $Resolution
            Notes                 = $Notes
            Quiet                 = $true
        }
        if ($null -ne $FastTubeProcessId -and $FastTubeProcessId.Count -gt 0) {
            $ftParams['ProcessId'] = $FastTubeProcessId
        } else {
            $ftParams['ProcessName'] = $FastTubeProcessName
            $ftParams['AggregateMatchingProcesses'] = $true
        }

        $brParams = @{
            Scenario              = $mappedScenario
            CustomScenarioLabel   = $phase
            SampleCount           = $sampleCount
            IntervalSeconds       = $IntervalSeconds
            IncludeChildProcesses = $true
            VideoUrl              = $VideoUrl
            Resolution            = $Resolution
            Notes                 = $Notes
            Quiet                 = $true
        }
        if ($null -ne $BrowserProcessId -and $BrowserProcessId.Count -gt 0) {
            $brParams['ProcessId'] = $BrowserProcessId
        } else {
            $brParams['ProcessName'] = $resolvedBrowserName
            $brParams['AggregateMatchingProcesses'] = $true
        }

        $ftReport = & $measureScript @ftParams
        $brReport = & $measureScript @brParams

        $phaseResults.Add((Compare-ProcessReports -FastTubeReport $ftReport -BrowserReport $brReport -Phase $phase))
    }
}

$comparisonReport = [PSCustomObject]@{
    SchemaVersion   = '1.0'
    GeneratedAt     = (Get-Date).ToString('o')
    VideoUrl        = $VideoUrl
    Resolution      = $Resolution
    DurationSeconds = $DurationSeconds
    IntervalSeconds = $IntervalSeconds
    Notes           = $Notes
    Phases          = @($phaseResults.ToArray())
}

$markdownSummary = Format-ChromeComparisonSummary -Comparison $comparisonReport
$comparisonReport | Add-Member -MemberType NoteProperty -Name 'Markdown' -Value $markdownSummary

if (-not [string]::IsNullOrWhiteSpace($OutputJson)) {
    $parentDir = Split-Path -Parent $OutputJson
    if (-not [string]::IsNullOrWhiteSpace($parentDir) -and -not (Test-Path -LiteralPath $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }
    $comparisonReport | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $OutputJson -Encoding UTF8
}

if (-not [string]::IsNullOrWhiteSpace($OutputMarkdown)) {
    $parentDir = Split-Path -Parent $OutputMarkdown
    if (-not [string]::IsNullOrWhiteSpace($parentDir) -and -not (Test-Path -LiteralPath $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }
    Set-Content -LiteralPath $OutputMarkdown -Value ($markdownSummary + [Environment]::NewLine) -Encoding UTF8
}

if ($AsJson) {
    $comparisonReport | ConvertTo-Json -Depth 10
} else {
    if (-not $Quiet) {
        Write-Host ""
        Write-Host $markdownSummary
    }
    return $comparisonReport
}
