<!--
  raptrix-pslf-rs
  Copyright (c) 2026 Raptrix PowerFlow

  This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
  If a copy of the MPL was not distributed with this file, You can obtain one at
  https://mozilla.org/MPL/2.0/.
-->

# raptrix-pslf-rs

GE PSLF (`.epc` + `.dyd`) to Raptrix PowerFlow Interchange (`.rpf`) conversion — built for **large cases**, **deterministic** Arrow IPC output, and **modern grid** constructs (IBRs, rich metadata) while staying faithful to legacy PSLF.

**Exact sibling** of `raptrix-psse-rs` in architecture, CLI, error handling, logging, packaging, and code style. The goal is zero-surprise developer and user experience across the Raptrix converter family.

Part of the Raptrix PowerFlow ecosystem.

For production-scale deployments and the broader solver stack, contact **Raptrix PowerFlow** via the [GitHub organization](https://github.com/RaptrixPowerFlow).

## Ecosystem Repos

- [raptrix-cim-rs](https://github.com/RaptrixPowerFlow/raptrix-cim-rs) - Unlimited-size CIM to RPF converter suite (keeper of the schema).
- [raptrix-psse-rs](https://github.com/RaptrixPowerFlow/raptrix-psse-rs) - Unlimited-size PSS/E to RPF converter.
- [raptrix-pslf-rs](https://github.com/RaptrixPowerFlow/raptrix-pslf-rs) - Unlimited-size GE PSLF to RPF converter (this crate).
- [raptrix-studio](https://github.com/RaptrixPowerFlow/raptrix-studio) - Free unlimited RPF viewer/editor.

## Quick Start

```bash
raptrix-pslf-rs convert --epc my_case.epc --output my_case.rpf
raptrix-pslf-rs convert --epc my_case.epc --dyd my_case.dyd --output my_case_dynamic.rpf
raptrix-pslf-rs convert --epc my_case.epc --output my_case_expanded.rpf --transformer-mode expanded
raptrix-pslf-rs view --input my_case.rpf
```

## CLI Reference (identical surface to psse-rs)

### convert

```bash
raptrix-pslf-rs convert --epc <FILE> [--dyd <FILE>] --output <FILE> [--transformer-mode <MODE>] [--study-purpose <TEXT>] [--scenario-tag <TAG> ...] [--case-mode <MODE>] [--default-shunt-control-mode <MODE>]
```

| Flag | Required | Description |
|------|----------|-------------|
| `--epc <PATH>` | yes | GE PSLF EPC file (.epc / .EPC). |
| `--dyd <PATH>` | no | Optional dynamic data file (.dyd). |
| `--output <PATH>` | yes | Output RPF path. |
| `--transformer-mode <MODE>` | no | `native-3w` (default) or `expanded` (mirrors psse-rs). |
| `--study-purpose <TEXT>` | no | Metadata override for `metadata.study_purpose`. |
| `--scenario-tag <TAG>` | no | Repeatable metadata override for `metadata.scenario_tags`. |
| `--case-mode <MODE>` | no | Optional override (`flat_start_planning`, `warm_start_planning`, `solved_snapshot`, `hour_ahead_advisory`). |
| `--default-shunt-control-mode <MODE>` | no | Optional override for shunt control mode handoff. |

### view

Pretty-prints table row counts from an existing `.rpf`.

### validate

Runs basic structural checks on an `.epc` (parse success + required table presence). Use `--strict` for CI exit codes.

## Fidelity & Modern Grid Support

- Emits the full set of **18** canonical required RPF **v0.12.1** root tables (zero-row where appropriate).
- IBR classification driven from `.dyd` model records (GENROU family + REPC_A / equivalent, matching psse-rs DYR logic where possible).
- Deterministic `case_fingerprint`, `bus_uuid` generation, and slack selection.
- Same sanitization and interchange-boundary rules as the PSS/E sibling.
- Full post-write contract validation via `raptrix-cim-arrow::validate_rpf_file`.

See `docs/pslf-mapping.md` for the authoritative field-by-field translation rules (to be expanded as the parser matures).

## Test Data (Important — CEII / Proprietary)

The reference test cases live in `tests/networks/` (Texas synthetic grids used for cross-validation with psse-rs).

**These files must never be committed to GitHub.** They are protected by a strict `.gitignore` and are only present on authorized developer machines.

**Primary regression signal**: For any grid that exists in both formats, the RPF produced from the `.epc` (this tool) must be row-count and aggregate equivalent to the RPF produced from the corresponding `.raw` (psse-rs).

## Cross-Repo Power Flow Integration

End-to-end workflow for comparing PSLF- and PSS/E-derived RPF files under **raptrix-core** Newton–Raphson (PV/PQ modes):

### 1. Generate aligned RPF pairs

```powershell
# Windows (PSLF via WSL build; PSSE native)
pwsh scripts/compare-psse-rpf.ps1

# Linux / Git Bash
bash scripts/compare-psse-rpf.sh
```

Both scripts pass `--case-mode warm_start_planning --default-shunt-control-mode planning_full` so export metadata matches between converters. Output:

- `tests/compare/pslf/{case}.rpf`
- `tests/compare/psse/{case}.rpf`

Structural diff: `compare_rpf left.rpf right.rpf` (built with this crate).

### 2. Solve and compare in raptrix-core

```powershell
cd ..\raptrix-core
$env:PYTHONPATH = (Get-Location)
.\.venv\Scripts\python.exe python_tests\regression\pslf_psse_rpf_parity_harness.py `
  --pslf-dir ..\raptrix-pslf-rs\tests\compare\pslf `
  --psse-dir ..\raptrix-pslf-rs\tests\compare\psse `
  --log python_tests\regression\out\pslf_psse_parity.jsonl

.\.venv\Scripts\python.exe python_tests\regression\parse_pslf_psse_log.py `
  --log python_tests\regression\out\pslf_psse_parity.jsonl `
  --output python_tests\regression\out\pslf_psse_parity_report.md
```

### Known PSLF vs PSS/E differences (v0.5.3)

These are **real format semantics**, not missing parser rows on core tables:

| Topic | PSLF-native | PSS/E-native | PF impact |
|-------|-------------|--------------|-----------|
| **Fixed shunts** | Texas7k EPC has `shunt data [0]`; no explicit table rows | 205 `fixed_shunts` rows | **Resolved for Texas7k** — bus `b_shunt` matches via SVD `b_init_pu`; table row-count still differs |
| **SVD bank granularity** | One row per step (`switched_shunt_banks`) | Often compressed banks | Low if bus-level SVD counts match |
| **Bus Q limits** | PV bus `q_min`/`q_max` often 0 in export | Aggregated from online gens | May affect post-solve parity vs PSSE (Q enforcement) |
| **Dynamics** | DYD model count ≠ DYR count | Different vendor formats | None for steady-state PF |

Core network tables (buses, branches, generators, loads, transformers_2w, areas, zones, owners) match row counts on Texas7k. Spot checks on bus 110001 voltages and gen 111180 MW match between converters.

**Windows note:** If `cargo build` fails with Application Control (error 4551) under OneDrive, build via WSL: `wsl -e bash -lc 'cd /mnt/c/.../raptrix-pslf-rs && cargo build --release'`, or set `CARGO_TARGET_DIR=C:\temp\raptrix-pslf-rs-target`.

## Building & Releasing

See the scripts/ directory (copied from the psse-rs sibling):

- `scripts/pre-release-check.ps1`
- `scripts/sync-versions.ps1`
- `scripts/package-windows.ps1` / `package-unix.sh`

Standard Rust commands work:

```bash
cargo build --release
cargo test
```

## Versioning & Schema Contract

This crate is pinned to **`raptrix-cim-arrow` 0.5.3** (git rev `298f9958cb9a551e273257f045bcadc1c72cf7bb`), matching the current `raptrix-psse-rs` release. Every emitted `.rpf` is validated against the locked **v0.12.1** contract before returning. Prior contract versions are not accepted by readers in `raptrix-cim-arrow` 0.5.3+ — re-emit cached files through this converter.

See [raptrix-cim-rs schema-contract](https://github.com/RaptrixPowerFlow/raptrix-cim-rs/blob/main/docs/schema-contract.md) for the full RPF specification.

## License

MPL-2.0. See `LICENSE`.

Copyright (c) 2026 Raptrix PowerFlow.

---

**Raptrix — We close the physics gap — planning to real time.**
