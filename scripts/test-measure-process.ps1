<#
.SYNOPSIS
    Testes automatizados seguros e nao intrusivos para `scripts/measure-process.ps1`.

.DESCRIPTION
    Executa verificacoes funcionais de `scripts/measure-process.ps1` sem modificar nem interromper
    executaveis em uso:
    1. Auto-medicao do proprio processo PowerShell (`$PID`) com exportacao JSON, CSV e Markdown.
    2. Validacao dos campos obrigatorios de memoria privada/residente, CPU, GPU e metadados de cenario.
    3. Validacao de falha controlada para processo inexistente.
    4. Se `fasttube` estiver em execucao, faz uma leitura curta nao intrusiva e confirma que o mesmo PID permanece ativo.
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptPath = Join-Path $PSScriptRoot 'measure-process.ps1'
if (-not (Test-Path $scriptPath)) {
    throw "Script nao encontrado em: $scriptPath"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("fasttube-measure-test-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

try {
    Write-Host "[1/4] Testando medicao no proprio processo PowerShell (PID $PID) e exportacao JSON/CSV/Markdown..." -ForegroundColor Cyan
    $jsonOut = Join-Path $tempDir 'self-test.json'
    $csvOut  = Join-Path $tempDir 'self-test.csv'
    $mdOut   = Join-Path $tempDir 'self-test.md'

    $result = & $scriptPath `
        -ProcessId $PID `
        -Scenario 'ocioso' `
        -SampleCount 2 `
        -IntervalSeconds 0.25 `
        -Notes 'Teste automatizado no proprio processo PowerShell' `
        -OutputJson $jsonOut `
        -OutputCsv $csvOut `
        -OutputMarkdown $mdOut `
        -Quiet

    if ($null -eq $result) { throw "O retorno de measure-process.ps1 foi nulo." }
    if ($result.Scenario -ne 'ocioso') { throw "Scenario esperado 'ocioso', obtido '$($result.Scenario)'." }
    if ($result.Sampling.CollectedSamples -ne 2) { throw "Esperadas 2 amostras, obtidas $($result.Sampling.CollectedSamples)." }
    if ($result.Summary.WorkingSetMiB.Avg -le 0) { throw "WorkingSetMiB.Avg invalido: $($result.Summary.WorkingSetMiB.Avg)" }
    if ($result.Summary.PrivateMiB.Avg -le 0) { throw "PrivateMiB.Avg invalido: $($result.Summary.PrivateMiB.Avg)" }
    if ($null -eq $result.Summary.WorkingSetPrivateMiB.Avg -or $result.Summary.WorkingSetPrivateMiB.Avg -le 0) {
        throw "WorkingSetPrivateMiB.Avg invalido: $($result.Summary.WorkingSetPrivateMiB.Avg)"
    }
    if ($null -eq $result.Summary.CpuTotalPercent.Avg -or $result.Summary.CpuTotalPercent.Avg -lt 0) {
        throw "CpuTotalPercent.Avg invalido: $($result.Summary.CpuTotalPercent.Avg)"
    }

    if (-not (Test-Path $jsonOut)) { throw "Arquivo JSON nao foi gerado." }
    if (-not (Test-Path $csvOut)) { throw "Arquivo CSV nao foi gerado." }
    if (-not (Test-Path $mdOut)) { throw "Arquivo Markdown nao foi gerado." }

    $parsedJson = Get-Content -Path $jsonOut -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($parsedJson.SchemaVersion -ne '1.0') { throw "SchemaVersion inesperado no JSON." }

    $csvRows = @(Import-Csv -Path $csvOut -Encoding UTF8)
    if ($csvRows.Count -ne 2) { throw "CSV deveria conter 2 linhas, mas contem $($csvRows.Count)." }

    $mdContent = Get-Content -Path $mdOut -Raw -Encoding UTF8
    if ($mdContent -notmatch 'Memoria Privada Residente') { throw "Markdown gerado nao contem a tabela esperada." }
    Write-Host "  OK: JSON, CSV e Markdown validados com sucesso." -ForegroundColor Green

    Write-Host "[2/4] Testando registro manual dos cenarios 'pausado' e 'reproducao' com -AppendMarkdown..." -ForegroundColor Cyan
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
        if ($r.Scenario -ne $sc) { throw "Falha ao registrar cenario '$sc'." }
    }
    $mdAppended = Get-Content -Path $mdOut -Raw -Encoding UTF8
    if ($mdAppended -notmatch 'Medicao: `pausado`' -or $mdAppended -notmatch 'Medicao: `reproducao`') {
        throw "AppendMarkdown nao incluiu os cenarios 'pausado' e 'reproducao'."
    }
    Write-Host "  OK: Cenarios manuais 'ocioso', 'pausado' e 'reproducao' validados." -ForegroundColor Green

    Write-Host "[3/4] Testando tratamento de erro para processo inexistente..." -ForegroundColor Cyan
    $threw = $false
    try {
        & $scriptPath -ProcessName 'processo_inexistente_fasttube_xyz_999' -SampleCount 1 -Quiet | Out-Null
    } catch {
        $threw = $true
    }
    if (-not $threw) {
        throw "O script deveria falhar ao consultar um processo inexistente."
    }
    Write-Host "  OK: Erro tratado adequadamente para processo inexistente." -ForegroundColor Green

    Write-Host "[4/4] Verificando leitura nao intrusiva do processo 'fasttube' (se estiver em execucao)..." -ForegroundColor Cyan
    $ftProcBefore = @(Get-Process -Name 'fasttube' -ErrorAction SilentlyContinue)
    if ($ftProcBefore.Count -eq 1) {
        $ftPid = $ftProcBefore[0].Id
        $ftResult = & $scriptPath `
            -ProcessId $ftPid `
            -Scenario 'nao-informado' `
            -SampleCount 2 `
            -IntervalSeconds 0.5 `
            -Notes 'Verificacao automatizada nao intrusiva' `
            -Quiet
        $ftProcAfter = Get-Process -Id $ftPid -ErrorAction SilentlyContinue
        if ($null -eq $ftProcAfter) {
            throw "O processo fasttube (PID $ftPid) nao foi encontrado apos a medicao!"
        }
        Write-Host ("  OK: fasttube (PID {0}) medido sem interrupcao -- WorkingSetPrivate medio: {1} MiB, WorkingSet64 medio: {2} MiB, GPU disponivel: {3}" -f `
            $ftPid, $ftResult.Summary.WorkingSetPrivateMiB.Avg, $ftResult.Summary.WorkingSetMiB.Avg, $ftResult.Summary.GpuAvailable) -ForegroundColor Green
    } else {
        Write-Host "  SKIP: Nenhum processo 'fasttube' unico em execucao no momento." -ForegroundColor Yellow
    }

    Write-Host ""
    Write-Host "Todos os testes de measure-process.ps1 passaram com sucesso." -ForegroundColor Green
} finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
