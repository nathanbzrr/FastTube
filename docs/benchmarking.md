# Benchmarking & Portable Packaging

## Portable Distribution

Build a standalone portable bundle (does not download third-party binaries or bundle user data/credentials):

```powershell
.\scripts\build-portable.ps1 `
  -LibMpvDirectory C:\path\to\mpv-dev `
  -BundleDirectory .\target\portable-bundle `
  -YtDlpPath .\yt-dlp.exe
```

Verify packaging behavior with fixture checks:

```powershell
.\scripts\test-portable-package.ps1
```

## Process Resource Sampling

[`scripts/measure-process.ps1`](../scripts/measure-process.ps1) samples an already-running Windows process non-intrusively (resident private working set, total working set, commit bytes, normalized CPU, WDDM GPU engine/memory counters, and child processes such as `yt-dlp.exe`):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-process.ps1 `
  -ProcessName fasttube `
  -Scenario ocioso `
  -SampleCount 15 `
  -IntervalSeconds 1.0 `
  -IncludeChildren $true `
  -OutputJson .\target\measurements\process.json `
  -OutputCsv .\target\measurements\process.csv
```

Supported `-Scenario` values: `nao-informado`, `ocioso`, `pausado`, `reproducao`, `pesquisa`, `comparacao-navegador`, `outro`.

## Interface Network Sampling

[`scripts/measure-network.ps1`](../scripts/measure-network.ps1) samples `NetworkInterface.GetIPStatistics()` across active network interfaces without packet capture:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\measure-network.ps1 `
  -InterfaceName "Ethernet" `
  -SampleCount 15 `
  -IntervalSeconds 1.0 `
  -Scenario reproducao `
  -OutputJson .\target\measurements\network.json `
  -OutputCsv .\target\measurements\network.csv
```

> [!WARNING]
> `measure-network.ps1` measures **total traffic across the entire network interface** (including all background processes and OS services). It is not filtered by PID and cannot be attributed solely to FastTube.

## Script Self-Tests

```powershell
.\scripts\test-measure-process.ps1
.\scripts\test-measure-network.ps1
.\scripts\test-portable-package.ps1
```
