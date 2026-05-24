# raptrix-pslf-rs
# Copyright (c) 2026 Raptrix PowerFlow
#
# This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
# If a copy of the MPL was not distributed with this file, You can obtain one at
# https://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
    Keeps Cargo.toml version in sync with CHANGELOG.md (and optionally the psse-rs sibling).

.DESCRIPTION
    This is a minimal faithful copy of the psse-rs version. Expand as needed for full parity.

.PARAMETER Check
    Only verify; do not modify files. Exit 1 on mismatch.
#>

param(
    [switch]$Check
)

$ErrorActionPreference = "Stop"

$toml = Get-Content Cargo.toml -Raw
if ($toml -match 'version\s*=\s*"([^"]+)"') {
    $cargoVersion = $matches[1]
} else {
    Write-Error "Could not find version in Cargo.toml"
    exit 1
}

$changelog = Get-Content CHANGELOG.md -Raw
if ($changelog -match '##\s*\[([0-9]+\.[0-9]+\.[0-9]+)\]') {
    $changelogVersion = $matches[1]
} else {
    Write-Warning "No version header found in CHANGELOG.md yet (normal for early scaffold)."
    $changelogVersion = $null
}

Write-Host "Cargo.toml version : $cargoVersion"
Write-Host "CHANGELOG top version: $changelogVersion"

if ($Check) {
    if ($changelogVersion -and ($cargoVersion -ne $changelogVersion)) {
        Write-Error "Version mismatch: Cargo=$cargoVersion, CHANGELOG=$changelogVersion"
        exit 1
    }
    Write-Host "Version check passed."
    exit 0
}

Write-Host "Sync complete (or no action needed in scaffold phase)."
