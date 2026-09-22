# Compare local EPC decks with a same-stem RAW. Inputs are gitignored and are not named here.
# Windows twin of compare-psse-rpf.sh with aligned export metadata flags.
#
# A local EPC is compared when a same-stem RAW exists beside this checkout.
# Companion dynamics files are optional and are discovered by stem.
#
# Usage:
#   .\scripts\compare-psse-rpf.ps1
#   .\scripts\compare-psse-rpf.ps1 -SkipBuild
#   .\scripts\run-compare-psse-rpf.ps1 -SkipBuild
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

function ConvertTo-WslPath {
    param([string]$Path)
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    if ($resolved -match '^([A-Za-z]):\\(.*)$') {
        $drive = $Matches[1].ToLower()
        $rest = ($Matches[2] -replace '\\', '/')
        return "/mnt/$drive/$rest"
    }
    return ($resolved -replace '\\', '/')
}

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
    $PslfRootWsl = ConvertTo-WslPath $PslfRoot
    Write-Host "[build] raptrix-pslf-rs (WSL)..." -ForegroundColor Cyan
    wsl -e bash -lc "cd '$PslfRootWsl' && cargo build --release --bin raptrix-pslf-rs --bin compare_rpf" | Out-Host
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

$PslfRootWsl = ConvertTo-WslPath $PslfRoot
$PslfBinWsl = "$PslfRootWsl/target/release/raptrix-pslf-rs"
$CompareBinWsl = "$PslfRootWsl/target/release/compare_rpf"
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
    $EpcWsl = ConvertTo-WslPath $Epc
    $DydWsl = if ($Dyd -and (Test-Path $Dyd)) { ConvertTo-WslPath $Dyd } else { $null }
    $PslfRpfWsl = ConvertTo-WslPath $PslfRpf

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
        $PsseRpfWsl = ConvertTo-WslPath $PsseRpf
        wsl -e bash -lc "$CompareBinWsl '$PslfRpfWsl' '$PsseRpfWsl'" | Out-Host
    }

    $ErrorActionPreference = $casePref
}

$NetworksDir = Join-Path $PslfRoot "tests\networks"
$ExternalDir = Join-Path $PsseRoot "tests\data\external"

function Find-Companion {
    param([string]$Dir, [string]$Stem, [string[]]$Extensions)
    if (-not (Test-Path $Dir)) { return $null }
    $files = @(Get-ChildItem -Path $Dir -File -ErrorAction SilentlyContinue)
    $exact = $files | Where-Object {
        $_.BaseName -eq $Stem -and $Extensions -contains $_.Extension.TrimStart('.').ToLowerInvariant()
    } | Select-Object -First 1
    if ($exact) { return $exact.FullName }
    $prefix = $files | Where-Object {
        $_.BaseName.StartsWith("${Stem}_") -and $Extensions -contains $_.Extension.TrimStart('.').ToLowerInvariant()
    } | Sort-Object { $_.Name.Length } | Select-Object -First 1
    if ($prefix) { return $prefix.FullName }
    return $null
}

$found = $false
if (Test-Path $NetworksDir) {
    $epcs = @(Get-ChildItem -Path $NetworksDir -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -match '^\.epc$' })
    foreach ($epc in $epcs) {
        $raw = Find-Companion $ExternalDir $epc.BaseName @("raw")
        if (-not $raw) { continue }
        $found = $true
        Compare-Case -Name $epc.BaseName -Epc $epc.FullName `
            -Dyd (Find-Companion $NetworksDir $epc.BaseName @("dyd")) `
            -Raw $raw `
            -Dyr (Find-Companion $ExternalDir $epc.BaseName @("dyr"))
    }
}
if (-not $found) {
    Write-Host "[skip] no local EPC with a same-stem RAW"
}


Write-Host ""
Write-Host "Done. RPF files written to:" -ForegroundColor Green
Write-Host "  $PslfOut"
Write-Host "  $PsseOut"
