# Generate PSLF and PSS/E RPF files for matching Texas cases and compare summaries.
# Windows twin of compare-psse-rpf.sh with aligned export metadata flags.
#
# Adding a new dual-format case:
#   1. Add EPC (+ optional DYD) under tests/networks/ and RAW (+ DYR) under
#      raptrix-psse-rs/tests/data/external/ with the same stem.
#   2. Append a stanza to the $Cases array below (and the shell script twin).
#   3. Add the stem to DEFAULT_CASES in
#      raptrix-core/python_tests/regression/pslf_psse_rpf_parity_harness.py.
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$PslfRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$PsseRoot = Resolve-Path (Join-Path $PslfRoot "..\raptrix-psse-rs")
$OutDir = Join-Path $PslfRoot "tests\compare"
$PslfOut = Join-Path $OutDir "pslf"
$PsseOut = Join-Path $OutDir "psse"

New-Item -ItemType Directory -Force -Path $PslfOut, $PsseOut | Out-Null

$ExportFlags = @(
    "--case-mode", "warm_start_planning",
    "--default-shunt-control-mode", "planning_full"
)

if (-not $SkipBuild) {
    $buildPref = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    Write-Host "[build] raptrix-pslf-rs (WSL)..." -ForegroundColor Cyan
    wsl -e bash -lc "cd /mnt/c/Users/matth/OneDrive/repos/raptrix-pslf-rs && cargo build --release --bin raptrix-pslf-rs --bin compare_rpf" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "pslf-rs WSL build failed" }

    if (-not $env:CARGO_TARGET_DIR) {
        $env:CARGO_TARGET_DIR = "C:\temp\raptrix-psse-rs-target"
    }
    Write-Host "[build] raptrix-psse-rs (target=$env:CARGO_TARGET_DIR)..." -ForegroundColor Cyan
    Push-Location $PsseRoot
    try {
        & cargo build --release 2>&1 | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "psse-rs build failed" }
    } finally {
        Pop-Location
    }
    $ErrorActionPreference = $buildPref
}

$PslfBinWsl = "/mnt/c/Users/matth/OneDrive/repos/raptrix-pslf-rs/target/release/raptrix-pslf-rs"
$CompareBinWsl = "/mnt/c/Users/matth/OneDrive/repos/raptrix-pslf-rs/target/release/compare_rpf"
$PsseExe = Join-Path $env:CARGO_TARGET_DIR "release\raptrix-psse-rs.exe"
if (-not (Test-Path $PsseExe)) {
    $PsseExe = Join-Path $PsseRoot "target\release\raptrix-psse-rs.exe"
}
if (-not (Test-Path $PsseExe)) {
    throw "raptrix-psse-rs.exe not found (rebuild required)"
}

function Compare-Case {
    param(
        [string]$Name,
        [string]$Epc,
        [string]$Dyd,
        [string]$Raw,
        [string]$Dyr
    )

    $casePref = $ErrorActionPreference
    $ErrorActionPreference = "Continue"

    Write-Host ""
    Write-Host "========================================" -ForegroundColor Yellow
    Write-Host " $Name" -ForegroundColor Yellow
    Write-Host "========================================" -ForegroundColor Yellow

    if (-not (Test-Path $Epc)) {
        Write-Host "[skip] missing EPC: $Epc" -ForegroundColor DarkYellow
        return
    }
    if (-not (Test-Path $Raw)) {
        Write-Host "[skip] missing RAW: $Raw" -ForegroundColor DarkYellow
        return
    }

    $PslfRpf = Join-Path $PslfOut "$Name.rpf"
    $PsseRpf = Join-Path $PsseOut "$Name.rpf"
    $EpcWsl = ($Epc -replace '\\', '/') -replace '^C:', '/mnt/c' -replace '^c:', '/mnt/c'
    $DydWsl = if ($Dyd) { ($Dyd -replace '\\', '/') -replace '^C:', '/mnt/c' -replace '^c:', '/mnt/c' } else { $null }
    $PslfRpfWsl = ($PslfRpf -replace '\\', '/') -replace '^C:', '/mnt/c' -replace '^c:', '/mnt/c'

    $pslfArgs = @("convert", "--epc", $EpcWsl, "--output", $PslfRpfWsl) + $ExportFlags
    if ($Dyd -and (Test-Path $Dyd)) {
        $pslfArgs += @("--dyd", $DydWsl)
    }
    $pslfCmd = "$PslfBinWsl " + (($pslfArgs | ForEach-Object { "'$_'" }) -join ' ')
    wsl -e bash -lc $pslfCmd | Out-Host
    if ($LASTEXITCODE -ne 0) { Write-Host "[warn] PSLF convert exit=$LASTEXITCODE" -ForegroundColor Yellow }

    $psseArgs = @("convert", "--raw", $Raw, "--output", $PsseRpf) + $ExportFlags
    if ($Dyr -and (Test-Path $Dyr)) {
        $psseArgs += @("--dyr", $Dyr)
    }
    & $PsseExe @psseArgs 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) { Write-Host "[warn] PSSE convert exit=$LASTEXITCODE" -ForegroundColor Yellow }

    if (Test-Path $PslfRpf) {
        $pslfSize = (Get-Item $PslfRpf).Length
        Write-Host "[written] $PslfRpf ($pslfSize bytes)"
    }
    if (Test-Path $PsseRpf) {
        $psseSize = (Get-Item $PsseRpf).Length
        Write-Host "[written] $PsseRpf ($psseSize bytes)"
    }

    if ((Test-Path $PslfRpf) -and (Test-Path $PsseRpf)) {
        $PsseRpfWsl = ($PsseRpf -replace '\\', '/') -replace '^C:', '/mnt/c' -replace '^c:', '/mnt/c'
        wsl -e bash -lc "$CompareBinWsl '$PslfRpfWsl' '$PsseRpfWsl'" | Out-Host
    }

    $ErrorActionPreference = $casePref
}

$NetworksDir = Join-Path $PslfRoot "tests\networks"
$ExternalDir = Join-Path $PsseRoot "tests\data\external"

$Cases = @(
    @{
        Name = "Texas7k_20210804"
        Epc  = Join-Path $NetworksDir "Texas7k_20210804.EPC"
        Dyd  = Join-Path $NetworksDir "Texas7k_20210804.dyd"
        Raw  = Join-Path $ExternalDir "Texas7k_20210804.RAW"
        Dyr  = Join-Path $ExternalDir "Texas7k_20210804.dyr"
    },
    @{
        Name = "Texas2k_series25_case1_summerpeak"
        Epc  = Join-Path $NetworksDir "Texas2k_series25_case1_summerpeak.EPC"
        Dyd  = Join-Path $NetworksDir "Texas2k_series25_case1_summerpeak.dyd"
        Raw  = Join-Path $ExternalDir "Texas2k_series25_case1_summerpeak.RAW"
        Dyr  = Join-Path $ExternalDir "Texas2k_series25_case1_summerpeak.dyr"
    },
    @{
        Name = "Texas2k_series24_case3_2024summerpeak"
        Epc  = Join-Path $NetworksDir "Texas2k_series24_case3_2024summerpeak.EPC"
        Dyd  = Join-Path $NetworksDir "Texas2k_series24_case3_2024summerpeak.dyd"
        Raw  = Join-Path $ExternalDir "Texas2k_series24_case3_2024summerpeak.RAW"
        Dyr  = Join-Path $ExternalDir "Texas2k_series24_case3_2024summerpeak.dyr"
    },
    @{
        Name = "Texas2k_series24_case2_2016lowload"
        Epc  = Join-Path $NetworksDir "Texas2k_series24_case2_2016lowload.EPC"
        Dyd  = Join-Path $NetworksDir "Texas2k_series24_case2_2016lowload.dyd"
        Raw  = Join-Path $ExternalDir "Texas2k_series24_case2_2016lowload.RAW"
        Dyr  = Join-Path $ExternalDir "Texas2k_series24_case2_2016lowload.dyr"
    },
    @{
        Name = "Texas2k_series24_case6_2024lowloadwithgfm"
        Epc  = Join-Path $NetworksDir "Texas2k_series24_case6_2024lowloadwithgfm.EPC"
        Dyd  = Join-Path $NetworksDir "Texas2k_series24_case6_2024lowloadwithgfm.dyd"
        Raw  = Join-Path $ExternalDir "Texas2k_series24_case6_2024lowloadwithgfm.RAW"
        Dyr  = Join-Path $ExternalDir "Texas2k_series24_case6_2024lowloadwithgfm.dyr"
    },
    @{
        Name = "Texas2k_series24_case4_2024lowload"
        Epc  = Join-Path $NetworksDir "Texas2k_series24_case4_2024lowload.EPC"
        Dyd  = Join-Path $NetworksDir "Texas2k_series24_case4_2024lowload.dyd"
        Raw  = Join-Path $ExternalDir "Texas2k_series24_case4_2024lowload.RAW"
        Dyr  = Join-Path $ExternalDir "Texas2k_series24_case4_2024lowload.dyr"
    },
    @{
        Name = "Texas2k_series24_case1_2016summerPeak"
        Epc  = Join-Path $NetworksDir "Texas2k_series24_case1_2016summerPeak.EPC"
        Dyd  = Join-Path $NetworksDir "Texas2k_series24_case1_2016summerPeak.dyd"
        Raw  = Join-Path $ExternalDir "Texas2k_series24_case1_2016summerPeak.RAW"
        Dyr  = Join-Path $ExternalDir "Texas2k_series24_case1_2016summerPeak.dyr"
    },
    @{
        Name = "ACTIVSg10k"
        Epc  = Join-Path $NetworksDir "ACTIVSg10k.EPC"
        Dyd  = Join-Path $NetworksDir "ACTIVSg10k_dynamics.dyd"
        Raw  = Join-Path $ExternalDir "ACTIVSg10k.RAW"
        Dyr  = Join-Path $ExternalDir "ACTIVSg10k_dynamics.dyr"
    },
    @{
        Name = "ACTIVSg70k"
        Epc  = Join-Path $NetworksDir "ACTIVSg70k.EPC"
        Dyd  = Join-Path $NetworksDir "ACTIVSg70k_dynamics.dyd"
        Raw  = Join-Path $ExternalDir "ACTIVSg70k.RAW"
        Dyr  = Join-Path $ExternalDir "ACTIVSg70k_dynamics.dyr"
    }
)

foreach ($case in $Cases) {
    Compare-Case @case
}

Write-Host ""
Write-Host "Done. RPF files written to:" -ForegroundColor Green
Write-Host "  $PslfOut"
Write-Host "  $PsseOut"
