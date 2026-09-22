#!/usr/bin/env bash
# Compare local EPC decks with a same-stem RAW under the sibling converter checkout.
# Inputs are gitignored. This script does not name them.
set -euo pipefail

SKIP_BUILD=0
if [[ "${1:-}" == "--skip-build" ]]; then
  SKIP_BUILD=1
  shift
fi

source "$HOME/.cargo/env" 2>/dev/null || true
export PATH="$HOME/.cargo/bin:$PATH"

PSLF="$(cd "$(dirname "$0")/.." && pwd)"
PSSE="$(cd "$PSLF/../raptrix-psse-rs" && pwd)"
OUT="$PSLF/tests/compare"

mkdir -p "$OUT/pslf" "$OUT/psse"

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "[build] raptrix-pslf-rs..."
  (cd "$PSLF" && cargo build --release)
  echo "[build] raptrix-psse-rs..."
  (cd "$PSSE" && cargo build --release)
else
  echo "[skip-build] using existing release binaries"
fi

PSLF_BIN="$PSLF/target/release/raptrix-pslf-rs"
PSSE_BIN="$PSSE/target/release/raptrix-psse-rs"
COMPARE_BIN="$PSLF/target/release/compare_rpf"

compare_case() {
  local name="$1"
  local epc="$2"
  local dyd="$3"
  local raw="$4"
  local dyr="$5"

  echo ""
  echo "========================================"
  echo " $name"
  echo "========================================"

  if [[ ! -f "$epc" ]]; then
    echo "[skip] missing EPC: $epc"
    return 0
  fi
  if [[ ! -f "$raw" ]]; then
    echo "[skip] missing RAW: $raw"
    return 0
  fi

  local pslf_out="$OUT/pslf/${name}.rpf"
  local psse_out="$OUT/psse/${name}.rpf"
  local export_flags=(--case-mode warm_start_planning --default-shunt-control-mode planning_full)

  if [[ -f "$dyd" ]]; then
    "$PSLF_BIN" convert --epc "$epc" --dyd "$dyd" --output "$pslf_out" "${export_flags[@]}"
  else
    "$PSLF_BIN" convert --epc "$epc" --output "$pslf_out" "${export_flags[@]}"
  fi

  if [[ -f "$dyr" ]]; then
    "$PSSE_BIN" convert --raw "$raw" --dyr "$dyr" --output "$psse_out" "${export_flags[@]}"
  else
    "$PSSE_BIN" convert --raw "$raw" --output "$psse_out" "${export_flags[@]}"
  fi

  echo "[written] $pslf_out ($(stat -c%s "$pslf_out") bytes)"
  echo "[written] $psse_out ($(stat -c%s "$psse_out") bytes)"
  echo ""

  "$COMPARE_BIN" "$pslf_out" "$psse_out" || true
}

shopt -s nullglob
found=0
declare -A seen_stems=()
for epc in "$PSLF/tests/networks/"*.epc "$PSLF/tests/networks/"*.EPC; do
  [[ -f "$epc" ]] || continue
  stem="$(basename "$epc")"
  stem="${stem%.*}"
  if [[ -n "${seen_stems[$stem]:-}" ]]; then
    continue
  fi
  seen_stems[$stem]=1
  raw=""
  for cand in "$PSSE/tests/data/external/${stem}.RAW" "$PSSE/tests/data/external/${stem}.raw"; do
    if [[ -f "$cand" ]]; then
      raw="$cand"
      break
    fi
  done
  [[ -n "$raw" ]] || continue
  dyd=""
  for cand in "$PSLF/tests/networks/${stem}.dyd" "$PSLF/tests/networks/${stem}.DYD"; do
    if [[ -f "$cand" ]]; then
      dyd="$cand"
      break
    fi
  done
  if [[ -z "$dyd" ]]; then
    shortest=""
    for cand in "$PSLF/tests/networks/${stem}_"*.dyd "$PSLF/tests/networks/${stem}_"*.DYD; do
      [[ -f "$cand" ]] || continue
      if [[ -z "$shortest" || ${#cand} -lt ${#shortest} ]]; then
        shortest="$cand"
      fi
    done
    dyd="$shortest"
  fi
  dyr=""
  for cand in "$PSSE/tests/data/external/${stem}.dyr" "$PSSE/tests/data/external/${stem}.DYR"; do
    if [[ -f "$cand" ]]; then
      dyr="$cand"
      break
    fi
  done
  if [[ -z "$dyr" ]]; then
    shortest=""
    for cand in "$PSSE/tests/data/external/${stem}_"*.dyr "$PSSE/tests/data/external/${stem}_"*.DYR; do
      [[ -f "$cand" ]] || continue
      if [[ -z "$shortest" || ${#cand} -lt ${#shortest} ]]; then
        shortest="$cand"
      fi
    done
    dyr="$shortest"
  fi
  found=1
  compare_case "$stem" "$epc" "$dyd" "$raw" "$dyr"
done
if [[ "$found" -eq 0 ]]; then
  echo "[skip] no local EPC with a same-stem RAW"
fi

echo ""
echo "Done. RPF files written to:"
echo "  $OUT/pslf/"
echo "  $OUT/psse/"
