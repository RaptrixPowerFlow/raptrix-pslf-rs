# raptrix-pslf-rs — keep Cargo.toml version aligned with CHANGELOG release heading.
param(
    [string]$Version,
    [switch]$Check
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-RootPackageVersion {
    $cargo = Get-Content -Raw "Cargo.toml"
    $m = [regex]::Match($cargo, '(?ms)^\[package\].*?^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"')
    if (-not $m.Success) { throw "Could not locate [package] version in Cargo.toml" }
    return $m.Groups[1].Value
}

if (-not $Version) {
    $Version = Get-RootPackageVersion
}

if (-not $Check) {
    Write-Host "sync-versions.ps1: use -Check in CI. Current Cargo.toml version: $Version"
    exit 0
}

$changelog = Get-Content -Raw "CHANGELOG.md"
$expected = "## [$Version]"
if ($changelog.IndexOf($expected, [System.StringComparison]::Ordinal) -lt 0) {
    Write-Error "CHANGELOG.md must contain release heading '$expected' for Cargo.toml version $Version."
    exit 1
}

Write-Host "Version consistency OK: Cargo.toml and CHANGELOG both reference $Version"
