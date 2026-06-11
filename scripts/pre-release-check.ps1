param(
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

Write-Host "[pre-release] checking version sync..."
./scripts/sync-versions.ps1 -Check

Write-Host "[pre-release] checking formatting..."
cargo fmt --all -- --check

if (Get-Command bash -ErrorAction SilentlyContinue) {
    Write-Host "[pre-release] public safety (tracked files — same as CI public-safety.yml)..."
    bash scripts/public-safety-check.sh --mode tracked
} else {
    Write-Warning "bash not on PATH; run ./scripts/public-safety-check.sh --mode tracked before publishing (or use WSL from the repo root)."
}

if (-not $SkipTests) {
    Write-Host "[pre-release] running tests..."
    cargo test --workspace
} else {
    Write-Host "[pre-release] tests skipped by request."
}

Write-Host "[pre-release] all checks passed."
