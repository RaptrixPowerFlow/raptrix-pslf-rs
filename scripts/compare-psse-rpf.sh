#!/usr/bin/env bash
# Generate PSLF and PSS/E RPF files for matching Texas cases and compare summaries.
#
# Adding a new dual-format case:
#   1. Add EPC (+ optional DYD) under tests/networks/ and RAW (+ DYR) under
#      ../raptrix-psse-rs/tests/data/external/ with the same stem.
#   2. Append a compare_case stanza below (and the PowerShell script twin).
#   3. Add the stem to DEFAULT_CASES in
#      ../raptrix-core/python_tests/regression/pslf_psse_rpf_parity_harness.py.
set -euo pipefail

source "$HOME/.cargo/env" 2>/dev/null || true
export PATH="$HOME/.cargo/bin:$PATH"

PSLF="$(cd "$(dirname "$0")/.." && pwd)"
PSSE="$(cd "$PSLF/../raptrix-psse-rs" && pwd)"
OUT="$PSLF/tests/compare"

mkdir -p "$OUT/pslf" "$OUT/psse"

echo "[build] raptrix-pslf-rs..."
(cd "$PSLF" && cargo build --release)
echo "[build] raptrix-psse-rs..."
(cd "$PSSE" && cargo build --release)

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

compare_case "Texas7k_20210804" \
  "$PSLF/tests/networks/Texas7k_20210804.EPC" \
  "$PSLF/tests/networks/Texas7k_20210804.dyd" \
  "$PSSE/tests/data/external/Texas7k_20210804.RAW" \
  "$PSSE/tests/data/external/Texas7k_20210804.dyr"

compare_case "Texas2k_series25_case1_summerpeak" \
  "$PSLF/tests/networks/Texas2k_series25_case1_summerpeak.EPC" \
  "$PSLF/tests/networks/Texas2k_series25_case1_summerpeak.dyd" \
  "$PSSE/tests/data/external/Texas2k_series25_case1_summerpeak.RAW" \
  "$PSSE/tests/data/external/Texas2k_series25_case1_summerpeak.dyr"

compare_case "Texas2k_series24_case3_2024summerpeak" \
  "$PSLF/tests/networks/Texas2k_series24_case3_2024summerpeak.EPC" \
  "$PSLF/tests/networks/Texas2k_series24_case3_2024summerpeak.dyd" \
  "$PSSE/tests/data/external/Texas2k_series24_case3_2024summerpeak.RAW" \
  "$PSSE/tests/data/external/Texas2k_series24_case3_2024summerpeak.dyr"

compare_case "Texas2k_series24_case2_2016lowload" \
  "$PSLF/tests/networks/Texas2k_series24_case2_2016lowload.EPC" \
  "$PSLF/tests/networks/Texas2k_series24_case2_2016lowload.dyd" \
  "$PSSE/tests/data/external/Texas2k_series24_case2_2016lowload.RAW" \
  "$PSSE/tests/data/external/Texas2k_series24_case2_2016lowload.dyr"

compare_case "Texas2k_series24_case6_2024lowloadwithgfm" \
  "$PSLF/tests/networks/Texas2k_series24_case6_2024lowloadwithgfm.EPC" \
  "$PSLF/tests/networks/Texas2k_series24_case6_2024lowloadwithgfm.dyd" \
  "$PSSE/tests/data/external/Texas2k_series24_case6_2024lowloadwithgfm.RAW" \
  "$PSSE/tests/data/external/Texas2k_series24_case6_2024lowloadwithgfm.dyr"

compare_case "Texas2k_series24_case4_2024lowload" \
  "$PSLF/tests/networks/Texas2k_series24_case4_2024lowload.EPC" \
  "$PSLF/tests/networks/Texas2k_series24_case4_2024lowload.dyd" \
  "$PSSE/tests/data/external/Texas2k_series24_case4_2024lowload.RAW" \
  "$PSSE/tests/data/external/Texas2k_series24_case4_2024lowload.dyr"

compare_case "Texas2k_series24_case1_2016summerPeak" \
  "$PSLF/tests/networks/Texas2k_series24_case1_2016summerPeak.EPC" \
  "$PSLF/tests/networks/Texas2k_series24_case1_2016summerPeak.dyd" \
  "$PSSE/tests/data/external/Texas2k_series24_case1_2016summerPeak.RAW" \
  "$PSSE/tests/data/external/Texas2k_series24_case1_2016summerPeak.dyr"

compare_case "ACTIVSg10k" \
  "$PSLF/tests/networks/ACTIVSg10k.EPC" \
  "$PSLF/tests/networks/ACTIVSg10k_dynamics.dyd" \
  "$PSSE/tests/data/external/ACTIVSg10k.RAW" \
  "$PSSE/tests/data/external/ACTIVSg10k_dynamics.dyr"

compare_case "ACTIVSg70k" \
  "$PSLF/tests/networks/ACTIVSg70k.EPC" \
  "$PSLF/tests/networks/ACTIVSg70k_dynamics.dyd" \
  "$PSSE/tests/data/external/ACTIVSg70k.RAW" \
  "$PSSE/tests/data/external/ACTIVSg70k_dynamics.dyr"

echo ""
echo "Done. RPF files written to:"
echo "  $OUT/pslf/"
echo "  $OUT/psse/"
