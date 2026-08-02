#!/usr/bin/env bash
# Regenerate golden-path .rpf files under tests/golden/ from tests/networks/.
# Canonical <stem>.rpf includes DYD when a companion exists (dynamic-first policy).
# Also emits <stem>_dynamic.rpf / <stem>_static.rpf twins (parity with raptrix-psse-rs).
# Repo-relative only (safe on any clone path). Prefer WSL on Windows + OneDrive.
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=/dev/null
. "$HOME/.cargo/env" 2>/dev/null || true

if [[ ! -d tests/networks ]]; then
  echo "[skip] tests/networks/ not present — proprietary EPC/DYD corpus is local-only."
  exit 0
fi

echo "[build] cargo build --release"
cargo build --release

echo "[golden] cargo test --release --test golden_test -- --nocapture"
cargo test --release --test golden_test -- --nocapture

echo
echo "[suite] finished — RPF schema stamped by raptrix-cim-arrow (single IPC writer)"
echo "[suite] policy: canonical <stem>.rpf includes DYD when a companion exists"
echo "[suite] aliases: <stem>_dynamic.rpf / <stem>_static.rpf under tests/golden/"
