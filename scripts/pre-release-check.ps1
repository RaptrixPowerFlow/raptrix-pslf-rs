# raptrix-pslf-rs
# Copyright (c) 2026 Raptrix PowerFlow
#
# This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
# If a copy of the MPL was not distributed with this file, You can obtain one at
# https://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
    Pre-release hygiene checks (format, tests, version sync, safety).
    Faithful minimal copy from psse-rs sibling. Expand for full parity.
#>

$ErrorActionPreference = "Stop"

Write-Host "=== raptrix-pslf-rs pre-release checks ==="

Write-Host "`n[1/5] cargo fmt --check"
cargo fmt --check

Write-Host "`n[2/5] cargo clippy (if available)"
cargo clippy -- -D warnings 2>$null || Write-Host "  (clippy not enforced in early scaffold)"

Write-Host "`n[3/5] cargo test (may skip proprietary data)"
cargo test -- --test-threads=1 2>&1 | Select-String -Pattern "(test result|FAILED|error)" -Context 0,1

Write-Host "`n[4/5] version sync check"
& "$PSScriptRoot\sync-versions.ps1" -Check

Write-Host "`n[5/5] public-safety-check (placeholder)"
if (Test-Path "$PSScriptRoot\public-safety-check.sh") {
    bash "$PSScriptRoot\public-safety-check.sh"
} else {
    Write-Host "  (public-safety-check.sh not present yet)"
}

Write-Host "`n=== Pre-release checks completed (scaffold phase) ==="
