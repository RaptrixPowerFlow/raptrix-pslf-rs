# Robust entry point for compare-psse-rpf.ps1 (avoids -Command param() escaping issues).
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
& (Join-Path $PSScriptRoot "compare-psse-rpf.ps1") @PSBoundParameters
