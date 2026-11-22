<#
.SYNOPSIS
    Safe, non-intrusive automated tests for `scripts/measure-process.ps1` and `scripts/compare-chrome.ps1`.

.DESCRIPTION
    Runs functional live checks and offline unit tests without modifying or terminating running apps:
    1. Self-measurement of the current PowerShell process (`$PID`) with JSON, CSV, and Markdown export
       (including new tree metrics: ChildProcessCount, TreeCpuPercent, TreeWorkingSetMB, TreePrivateMB, TreeGpuPercent).
    2. Manual scenario logging ('pausado' and 'reproducao') with `-AppendMarkdown`.
    3. Controlled error handling for a non-existent process.
    4. Non-intrusive check against `fasttube` if a single instance is running.
    5. Offline unit tests for `Get-ProcessTreePids` (multi-level trees, multiple roots, PID-reuse cycle guards).
    6. Offline unit tests for `Aggregate-ProcessTreeSample` (root + child RAM/CPU summing, pre-existing vs new child CPU delta).
    7. Offline unit tests for `Parse-GpuEngineUtilization` (Windows GPU Engine counter parsing, WMI format, unavailable fallback).
    8. Offline unit tests for `compare-chrome.ps1` (`Compare-ProcessReports`, `Format-ChromeComparisonSummary`, and `-DryRunFixture`).
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptPath = Join-Path $PSScriptRoot 'measure-process.ps1'
$compareScriptPath = Join-Path $PSScriptRoot 'compare-chrome.ps1'
if (-not (Test-Path $scriptPath)) {
    throw "Script not found at: $scriptPath"
}
if (-not (Test-Path $compareScriptPath)) {
    throw "Comparative script not found at: $compareScriptPath"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("fasttube-measure-test-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

try {
    Write-Host "[1/8] Testing self-measurement on PowerShell process (PID $PID) and JSON/CSV/Markdown export..." -ForegroundColor Cyan
    $jsonOut = Join-Path $tempDir 'self-test.json'
    $csvOut  = Join-Path $tempDir 'self-test.csv'
    $mdOut   = Join-Path $tempDir 'self-test.md'

    $result = & $scriptPath `
        -ProcessId $PID `
        -Scenario 'ocioso' `
        -SampleCount 2 `
        -IntervalSeconds 0.25 `
        -IncludeChildProcesses `
        -Notes 'Automated self-test on current PowerShell process' `
        -OutputJson $jsonOut `
        -OutputCsv $csvOut `
        -OutputMarkdown $mdOut `
        -Quiet

    if ($null -eq $result) { throw "measure-process.ps1 returned null." }
    if ($result.Scenario -ne 'ocioso') { throw "Expected scenario 'ocioso', got '$($result.Scenario)'." }
    if ($result.Sampling.CollectedSamples -ne 2) { throw "Expected 2 samples, got $($result.Sampling.CollectedSamples)." }
    if ($result.Summary.WorkingSetMiB.Avg -le 0) { throw "Invalid WorkingSetMiB.Avg: $($result.Summary.WorkingSetMiB.Avg)" }
    if ($result.Summary.PrivateMiB.Avg -le 0) { throw "Invalid PrivateMiB.Avg: $($result.Summary.PrivateMiB.Avg)" }
    if ($null -eq $result.Summary.WorkingSetPrivateMiB.Avg -or $result.Summary.WorkingSetPrivateMiB.Avg -le 0) {
        throw "Invalid WorkingSetPrivateMiB.Avg: $($result.Summary.WorkingSetPrivateMiB.Avg)"
    }
    if ($null -eq $result.Summary.CpuTotalPercent.Avg -or $result.Summary.CpuTotalPercent.Avg -lt 0) {
        throw "Invalid CpuTotalPercent.Avg: $($result.Summary.CpuTotalPercent.Avg)"
    }

    # Verify new process-tree fields exist on both Sample and Summary objects.
    $firstSample = $result.Samples[0]
    foreach ($requiredProp in @('ChildProcessCount', 'TreeCpuPercent', 'TreeWorkingSetMB', 'TreePrivateMB', 'TreeGpuPercent')) {
        if ($null -eq $firstSample.PSObject.Properties[$requiredProp]) {
            throw "Sample missing required tree property '$requiredProp'."
        }
        if ($null -eq $result.Summary.PSObject.Properties[$requiredProp]) {
            throw "Summary missing required tree property '$requiredProp'."
        }
    }
    if ($result.Summary.TreeWorkingSetMB.Avg -lt $result.Summary.WorkingSetMiB.Avg) {
        throw "TreeWorkingSetMB.Avg ($($result.Summary.TreeWorkingSetMB.Avg)) cannot be less than root WorkingSetMiB.Avg ($($result.Summary.WorkingSetMiB.Avg))."
    }
    if ($result.Summary.TreePrivateMB.Avg -lt $result.Summary.PrivateMiB.Avg) {
        throw "TreePrivateMB.Avg ($($result.Summary.TreePrivateMB.Avg)) cannot be less than root PrivateMiB.Avg ($($result.Summary.PrivateMiB.Avg))."
    }

    if (-not (Test-Path $jsonOut)) { throw "JSON output file was not created." }
    if (-not (Test-Path $csvOut)) { throw "CSV output file was not created." }
    if (-not (Test-Path $mdOut)) { throw "Markdown output file was not created." }

    $parsedJson = Get-Content -Path $jsonOut -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($parsedJson.SchemaVersion -ne '1.0') { throw "Unexpected SchemaVersion in JSON." }

    $csvRows = @(Import-Csv -Path $csvOut -Encoding UTF8)
    if ($csvRows.Count -ne 2) { throw "CSV should contain 2 rows, got $($csvRows.Count)." }

    $mdContent = Get-Content -Path $mdOut -Raw -Encoding UTF8
    if ($mdContent -notmatch 'Memoria Privada Residente') { throw "Generated Markdown does not contain expected table rows." }
    if ($mdContent -notmatch 'TreeWorkingSetMB') { throw "Generated Markdown missing TreeWorkingSetMB row." }
    Write-Host "  OK: JSON, CSV, Markdown, and tree summary fields validated." -ForegroundColor Green

    Write-Host "[2/8] Testing manual scenarios 'pausado' and 'reproducao' with -AppendMarkdown..." -ForegroundColor Cyan
    foreach ($sc in @('pausado', 'reproducao')) {
        $r = & $scriptPath `
            -ProcessId $PID `
            -Scenario $sc `
            -SampleCount 1 `
            -IntervalSeconds 0.2 `
            -VideoUrl 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' `
            -Resolution '1080p' `
            -OutputMarkdown $mdOut `
            -AppendMarkdown `
            -Quiet
        if ($r.Scenario -ne $sc) { throw "Failed to record scenario '$sc'." }
    }
    $mdAppended = Get-Content -Path $mdOut -Raw -Encoding UTF8
    if ($mdAppended -notmatch 'Medicao: `pausado`' -or $mdAppended -notmatch 'Medicao: `reproducao`') {
        throw "AppendMarkdown did not include 'pausado' and 'reproducao' sections."
    }
    Write-Host "  OK: Manual scenarios 'ocioso', 'pausado', and 'reproducao' validated." -ForegroundColor Green

    Write-Host "[3/8] Testing error handling for non-existent process..." -ForegroundColor Cyan
    $threw = $false
    try {
        & $scriptPath -ProcessName 'processo_inexistente_fasttube_xyz_999' -SampleCount 1 -Quiet | Out-Null
    } catch {
        $threw = $true
    }
    if (-not $threw) {
        throw "Expected script to fail when querying a non-existent process."
    }
    Write-Host "  OK: Non-existent process error handled properly." -ForegroundColor Green

    Write-Host "[4/8] Checking non-intrusive measurement on 'fasttube' (if running)..." -ForegroundColor Cyan
    $ftProcBefore = @(Get-Process -Name 'fasttube' -ErrorAction SilentlyContinue)
    if ($ftProcBefore.Count -eq 1) {
        $ftPid = $ftProcBefore[0].Id
        $ftResult = & $scriptPath `
            -ProcessId $ftPid `
            -Scenario 'nao-informado' `
            -SampleCount 2 `
            -IntervalSeconds 0.5 `
            -Notes 'Automated non-intrusive check' `
            -Quiet
        $ftProcAfter = Get-Process -Id $ftPid -ErrorAction SilentlyContinue
        if ($null -eq $ftProcAfter) {
            throw "fasttube process (PID $ftPid) disappeared after measurement!"
        }
        Write-Host ("  OK: fasttube (PID {0}) measured safely -- WorkingSetPrivate avg: {1} MiB, TreeWorkingSetMB avg: {2} MiB, GPU available: {3}" -f `
            $ftPid, $ftResult.Summary.WorkingSetPrivateMiB.Avg, $ftResult.Summary.TreeWorkingSetMB.Avg, $ftResult.Summary.TreeGpuAvailable) -ForegroundColor Green
    } else {
        Write-Host "  SKIP: No single 'fasttube' process currently running." -ForegroundColor Yellow
    }

    # Dot-source pure helpers from measure-process.ps1 and compare-chrome.ps1 for offline unit tests.
    . $scriptPath
    . $compareScriptPath

    Write-Host "[5/8] Offline unit tests: Get-ProcessTreePids (multi-level trees & PID-reuse cycle guards)..." -ForegroundColor Cyan
    $processTable = @(
        [PSCustomObject]@{ ProcessId = 100; ParentProcessId = 1;   Name = 'fasttube.exe' }
        [PSCustomObject]@{ ProcessId = 200; ParentProcessId = 100; Name = 'yt-dlp.exe' }
        [PSCustomObject]@{ ProcessId = 201; ParentProcessId = 100; Name = 'conhost.exe' }
        [PSCustomObject]@{ ProcessId = 300; ParentProcessId = 200; Name = 'ffmpeg.exe' }
        [PSCustomObject]@{ ProcessId = 400; ParentProcessId = 300; Name = 'helper.exe' }
        [PSCustomObject]@{ ProcessId = 900; ParentProcessId = 1;   Name = 'unrelated.exe' }
        [PSCustomObject]@{ ProcessId = 901; ParentProcessId = 900; Name = 'unrelated-child.exe' }
    )
    $fullTree = @(Get-ProcessTreePids -RootPids @(100) -ProcessTable $processTable | Sort-Object)
    if (($fullTree -join ',') -ne '100,200,201,300,400') {
        throw "Unexpected full process tree PIDs: $($fullTree -join ',')"
    }
    $descOnly = @(Get-ProcessTreePids -RootPids @(100) -ProcessTable $processTable -DescendantsOnly | Sort-Object)
    if (($descOnly -join ',') -ne '200,201,300,400') {
        throw "Unexpected descendants-only PIDs: $($descOnly -join ',')"
    }

    # Cycle guard test: root cycle (10 -> 20 -> 30 -> 10), self-parent (20 -> 20), and child cycle (50 <-> 60).
    $cyclicTable = @(
        [PSCustomObject]@{ ProcessId = 10; ParentProcessId = 30; Name = 'root.exe' }
        [PSCustomObject]@{ ProcessId = 20; ParentProcessId = 10; Name = 'workerA.exe' }
        [PSCustomObject]@{ ProcessId = 20; ParentProcessId = 20; Name = 'self-cycle.exe' }
        [PSCustomObject]@{ ProcessId = 30; ParentProcessId = 20; Name = 'workerB.exe' }
        [PSCustomObject]@{ ProcessId = 50; ParentProcessId = 30; Name = 'loopA.exe' }
        [PSCustomObject]@{ ProcessId = 60; ParentProcessId = 50; Name = 'loopB.exe' }
        [PSCustomObject]@{ ProcessId = 50; ParentProcessId = 60; Name = 'loopBack.exe' }
    )
    $cyclicTree = @(Get-ProcessTreePids -RootPids @(10) -ProcessTable $cyclicTable | Sort-Object)
    if (($cyclicTree -join ',') -ne '10,20,30,50,60') {
        throw "Cycle guard failed; unexpected PIDs: $($cyclicTree -join ',')"
    }
    Write-Host "  OK: Multi-level tree traversal and PID-reuse cycle guards verified." -ForegroundColor Green

    Write-Host "[6/8] Offline unit tests: Aggregate-ProcessTreeSample (CPU/RAM/child aggregation)..." -ForegroundColor Cyan
    $prevCpu = @{
        100 = 10.0
        200 = 2.0
    }
    $snapshots = @(
        [PSCustomObject]@{
            ProcessId                 = 100
            ProcessName               = 'fasttube'
            WorkingSetBytes           = [int64](150 * 1MB)
            PrivateBytes              = [int64](120 * 1MB)
            WorkingSetPrivateBytes    = [int64](110 * 1MB)
            TotalProcessorTimeSeconds = 10.4
            IsNewChild                = $false
        }
        [PSCustomObject]@{
            ProcessId                 = 200
            ProcessName               = 'yt-dlp'
            WorkingSetBytes           = [int64](40 * 1MB)
            PrivateBytes              = [int64](35 * 1MB)
            WorkingSetPrivateBytes    = [int64](30 * 1MB)
            TotalProcessorTimeSeconds = 2.6
            IsNewChild                = $false
        }
        [PSCustomObject]@{
            ProcessId                 = 300
            ProcessName               = 'ffmpeg'
            WorkingSetBytes           = [int64](60 * 1MB)
            PrivateBytes              = [int64](45 * 1MB)
            WorkingSetPrivateBytes    = [int64](40 * 1MB)
            TotalProcessorTimeSeconds = 0.6
            IsNewChild                = $true
        }
        [PSCustomObject]@{
            ProcessId                 = 400
            ProcessName               = 'preexisting-worker'
            WorkingSetBytes           = [int64](50 * 1MB)
            PrivateBytes              = [int64](40 * 1MB)
            WorkingSetPrivateBytes    = [int64](35 * 1MB)
            TotalProcessorTimeSeconds = 99.0
            IsNewChild                = $false
        }
    )

    # Delta wall = 2.0s, 4 logical cores.
    # Delta CPU: root(100)=0.4s + child(200)=0.6s + new child(300)=0.6s + preexisting unbaselined(400)=0.0s => 1.6s total.
    # TreeCpuPercent = (1.6 / 2.0 / 4) * 100 = 20.0%. ChildCpuPercent = (1.2 / 2.0 / 4) * 100 = 15.0%.
    $agg = Aggregate-ProcessTreeSample `
        -RootPids @(100) `
        -ProcessSnapshots $snapshots `
        -PreviousCpuSeconds $prevCpu `
        -DeltaWallSeconds 2.0 `
        -LogicalProcessors 4

    if ($agg.RootProcessCount -ne 1 -or $agg.ChildProcessCount -ne 3 -or $agg.TreeProcessCount -ne 4) {
        throw "Unexpected process counts: root=$($agg.RootProcessCount), child=$($agg.ChildProcessCount), tree=$($agg.TreeProcessCount)."
    }
    if ($agg.TreeWorkingSetMB -ne 300.0) { throw "Expected TreeWorkingSetMB=300.0, got $($agg.TreeWorkingSetMB)." }
    if ($agg.TreePrivateMB -ne 240.0) { throw "Expected TreePrivateMB=240.0, got $($agg.TreePrivateMB)." }
    if ($agg.TreeWorkingSetPrivateMB -ne 215.0) { throw "Expected TreeWorkingSetPrivateMB=215.0, got $($agg.TreeWorkingSetPrivateMB)." }
    if ($agg.TreeCpuPercent -ne 20.0) { throw "Expected TreeCpuPercent=20.0, got $($agg.TreeCpuPercent)." }
    if ($agg.ChildCpuPercent -ne 15.0) { throw "Expected ChildCpuPercent=15.0, got $($agg.ChildCpuPercent)." }
    if ($agg.TreeCpuSingleCorePercent -ne 80.0) { throw "Expected TreeCpuSingleCorePercent=80.0, got $($agg.TreeCpuSingleCorePercent)." }
    if (($agg.ChildProcessNames -join ',') -notmatch 'yt-dlp#200' -or ($agg.ChildProcessNames -join ',') -notmatch 'ffmpeg#300') {
        throw "ChildProcessNames missing expected entries: $($agg.ChildProcessNames -join ', ')"
    }
    if ($agg.NextCpuSeconds[400] -ne 99.0) { throw "NextCpuSeconds did not retain updated baseline for child PID 400." }
    Write-Host "  OK: Process-tree CPU/RAM/count/name aggregation verified." -ForegroundColor Green

    Write-Host "[7/8] Offline unit tests: Parse-GpuEngineUtilization (counter paths, WMI format & unavailable fallback)..." -ForegroundColor Cyan
    $gpuCounterSamples = @(
        [PSCustomObject]@{
            Path        = '\\TESTPC\gpu engine(pid_100_luid_0x00000000_0x00010f22_phys_0_eng_0_engtype_3D)\utilization percentage'
            CookedValue = 4.25
        }
        [PSCustomObject]@{
            Path        = '\\TESTPC\gpu engine(pid_100_luid_0x00000000_0x00010f22_phys_0_eng_2_engtype_VideoDecode)\utilization percentage'
            CookedValue = 8.50
        }
        [PSCustomObject]@{
            Path        = '\\TESTPC\gpu engine(pid_200_luid_0x00000000_0x00010f22_phys_0_eng_1_engtype_Copy)\utilization percentage'
            CookedValue = 1.75
        }
        [PSCustomObject]@{
            Path        = '\\TESTPC\gpu engine(pid_999_luid_0x00000000_0x00010f22_phys_0_eng_0_engtype_3D)\utilization percentage'
            CookedValue = 77.0
        }
    )
    $parsedGpu = Parse-GpuEngineUtilization -CounterSamples $gpuCounterSamples -TargetPids @(100, 200)
    if (-not $parsedGpu.Available) { throw "Expected GPU counters to be available." }
    if ($parsedGpu.TotalPercent -ne 14.5) { throw "Expected TotalPercent=14.5, got $($parsedGpu.TotalPercent)." }
    if ($parsedGpu.Engine3DPercent -ne 4.25) { throw "Expected Engine3DPercent=4.25, got $($parsedGpu.Engine3DPercent)." }
    if ($parsedGpu.VideoCodecPercent -ne 8.5) { throw "Expected VideoCodecPercent=8.5, got $($parsedGpu.VideoCodecPercent)." }
    if ($parsedGpu.CopyPercent -ne 1.75) { throw "Expected CopyPercent=1.75, got $($parsedGpu.CopyPercent)." }
    if ($parsedGpu.PerPidPercent[100] -ne 12.75 -or $parsedGpu.PerPidPercent[200] -ne 1.75) {
        throw "Unexpected PerPidPercent breakdown."
    }

    # WMI/CIM format compatibility check.
    $wmiSamples = @(
        [PSCustomObject]@{ Name = 'pid_555_luid_0x0_phys_0_eng_0_engtype_3D'; UtilizationPercentage = 0.0 }
    )
    $zeroGpu = Parse-GpuEngineUtilization -CounterSamples $wmiSamples -TargetPids @(555)
    if (-not $zeroGpu.Available -or $zeroGpu.TotalPercent -ne 0.0) {
        throw "Expected Available=true and TotalPercent=0.0 for zero-utilization matched counter."
    }

    # Graceful unavailable fallback checks.
    $nullGpu = Parse-GpuEngineUtilization -CounterSamples $null -TargetPids @(100)
    if ($nullGpu.Available -or $null -ne $nullGpu.TotalPercent) {
        throw "Expected Available=false and TotalPercent=null when counters are unavailable."
    }
    $unmatchedGpu = Parse-GpuEngineUtilization -CounterSamples $gpuCounterSamples -TargetPids @(8888)
    if ($unmatchedGpu.Available -or $null -ne $unmatchedGpu.TotalPercent) {
        throw "Expected Available=false and TotalPercent=null when no PIDs match."
    }
    Write-Host "  OK: GPU engine utilization parser and graceful fallback verified." -ForegroundColor Green

    Write-Host "[8/8] Offline unit tests: compare-chrome.ps1 (Compare-ProcessReports, Format-ChromeComparisonSummary & -DryRunFixture)..." -ForegroundColor Cyan
    $ftFixtureReport = [PSCustomObject]@{
        Target  = [PSCustomObject]@{ ProcessName = 'fasttube' }
        Summary = [PSCustomObject]@{
            TreeWorkingSetMB        = [PSCustomObject]@{ Avg = 160.0; P95 = 180.0 }
            TreePrivateMB           = [PSCustomObject]@{ Avg = 140.0; P95 = 155.0 }
            TreeWorkingSetPrivateMB = [PSCustomObject]@{ Avg = 130.0; P95 = 145.0 }
            TreeCpuPercent          = [PSCustomObject]@{ Avg = 2.5;   P95 = 4.0 }
            TreeGpuPercent          = [PSCustomObject]@{ Avg = 4.0;   P95 = 6.0 }
            TreeProcessCount        = [PSCustomObject]@{ Avg = 2.0;   Max = 3.0 }
        }
    }
    $brFixtureReport = [PSCustomObject]@{
        Target  = [PSCustomObject]@{ ProcessName = 'chrome' }
        Summary = [PSCustomObject]@{
            TreeWorkingSetMB        = [PSCustomObject]@{ Avg = 800.0; P95 = 920.0 }
            TreePrivateMB           = [PSCustomObject]@{ Avg = 700.0; P95 = 810.0 }
            TreeWorkingSetPrivateMB = [PSCustomObject]@{ Avg = 650.0; P95 = 750.0 }
            TreeCpuPercent          = [PSCustomObject]@{ Avg = 10.0;  P95 = 16.0 }
            TreeGpuPercent          = [PSCustomObject]@{ Avg = 16.0;  P95 = 24.0 }
            TreeProcessCount        = [PSCustomObject]@{ Avg = 14.0;  Max = 18.0 }
        }
    }

    $phaseCmp = Compare-ProcessReports -FastTubeReport $ftFixtureReport -BrowserReport $brFixtureReport -Phase 'playback-1080p'
    if ($phaseCmp.Metrics.WorkingSetMB.Avg.Delta -ne -640.0) { throw "Unexpected RAM delta: $($phaseCmp.Metrics.WorkingSetMB.Avg.Delta)" }
    if ($phaseCmp.Metrics.WorkingSetMB.Avg.ReductionPercent -ne 80.0) { throw "Unexpected RAM reduction%: $($phaseCmp.Metrics.WorkingSetMB.Avg.ReductionPercent)" }
    if ($phaseCmp.Metrics.WorkingSetMB.Avg.Factor -ne 5.0) { throw "Unexpected RAM factor: $($phaseCmp.Metrics.WorkingSetMB.Avg.Factor)" }
    if ($phaseCmp.Metrics.CpuPercent.Avg.ReductionPercent -ne 75.0) { throw "Unexpected CPU reduction%: $($phaseCmp.Metrics.CpuPercent.Avg.ReductionPercent)" }
    if ($phaseCmp.Metrics.GpuPercent.Avg.ReductionPercent -ne 75.0) { throw "Unexpected GPU reduction%: $($phaseCmp.Metrics.GpuPercent.Avg.ReductionPercent)" }
    if ($phaseCmp.Metrics.ProcessCount.Avg.Delta -ne -12.0) { throw "Unexpected ProcessCount delta: $($phaseCmp.Metrics.ProcessCount.Avg.Delta)" }

    $mdTable = Format-ChromeComparisonSummary -Comparison $phaseCmp
    if ($mdTable -notmatch 'Tree Working Set \(MB\)' -or $mdTable -notmatch '80%' -or $mdTable -notmatch '5x') {
        throw "Formatted comparison Markdown missing expected values."
    }

    # Test graceful N/A formatting when GPU counters are unavailable.
    $noGpuFt = [PSCustomObject]@{
        Target  = [PSCustomObject]@{ ProcessName = 'fasttube' }
        Summary = [PSCustomObject]@{
            TreeWorkingSetMB = [PSCustomObject]@{ Avg = 100.0; P95 = 110.0 }
            TreePrivateMB    = [PSCustomObject]@{ Avg = 90.0;  P95 = 95.0 }
            TreeCpuPercent   = [PSCustomObject]@{ Avg = 1.0;   P95 = 2.0 }
            TreeGpuPercent   = $null
            TreeProcessCount = [PSCustomObject]@{ Avg = 1.0;   Max = 1.0 }
        }
    }
    $noGpuBr = [PSCustomObject]@{
        Target  = [PSCustomObject]@{ ProcessName = 'msedge' }
        Summary = [PSCustomObject]@{
            TreeWorkingSetMB = [PSCustomObject]@{ Avg = 500.0; P95 = 550.0 }
            TreePrivateMB    = [PSCustomObject]@{ Avg = 450.0; P95 = 480.0 }
            TreeCpuPercent   = [PSCustomObject]@{ Avg = 4.0;   P95 = 6.0 }
            TreeGpuPercent   = $null
            TreeProcessCount = [PSCustomObject]@{ Avg = 10.0;  Max = 12.0 }
        }
    }
    $noGpuCmp = Compare-ProcessReports -FastTubeReport $noGpuFt -BrowserReport $noGpuBr -Phase 'idle'
    $noGpuMd = Format-ChromeComparisonSummary -Comparison $noGpuCmp
    if ($noGpuMd -notmatch '\| Tree GPU Utilization \(%\) \| N/A \| N/A \| N/A \| N/A \| N/A \|') {
        throw "Expected graceful N/A GPU row when GPU counters are null."
    }

    # End-to-end CLI invocation with -DryRunFixture.
    $fixturePath = Join-Path $tempDir 'compare-fixture.json'
    $cmpJsonOut = Join-Path $tempDir 'compare-out.json'
    $cmpMdOut = Join-Path $tempDir 'compare-out.md'
    [PSCustomObject]@{
        Phases = @(
            [PSCustomObject]@{ Phase = 'playback-1080p'; FastTube = $ftFixtureReport; Browser = $brFixtureReport }
            [PSCustomObject]@{ Phase = 'idle';           FastTube = $noGpuFt;         Browser = $noGpuBr }
        )
    } | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $fixturePath -Encoding UTF8

    $cmpCli = & $compareScriptPath `
        -DryRunFixture $fixturePath `
        -VideoUrl 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' `
        -Resolution '1080p' `
        -DurationSeconds 30 `
        -OutputJson $cmpJsonOut `
        -OutputMarkdown $cmpMdOut `
        -Quiet

    if ($cmpCli.Phases.Count -ne 2) { throw "Expected 2 phases in compare-chrome.ps1 output, got $($cmpCli.Phases.Count)." }
    if (-not (Test-Path $cmpJsonOut) -or -not (Test-Path $cmpMdOut)) {
        throw "compare-chrome.ps1 did not write JSON/Markdown output files."
    }
    $parsedCmpJson = Get-Content -LiteralPath $cmpJsonOut -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($parsedCmpJson.Phases.Count -ne 2) { throw "compare-chrome.ps1 JSON report phase count mismatch." }
    $writtenCmpMd = Get-Content -LiteralPath $cmpMdOut -Raw -Encoding UTF8
    if ($writtenCmpMd -notmatch 'playback-1080p' -or $writtenCmpMd -notmatch 'idle') {
        throw "compare-chrome.ps1 Markdown report missing phase headers."
    }
    Write-Host "  OK: compare-chrome.ps1 helpers and -DryRunFixture CLI verified." -ForegroundColor Green

    Write-Host ""
    Write-Host "All measure-process.ps1 and compare-chrome.ps1 tests passed." -ForegroundColor Green
} finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
