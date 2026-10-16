<!--
  raptrix-pslf-rs
  Copyright (c) 2026 Raptrix PowerFlow

  This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
  If a copy of the MPL was not distributed with this file, You can obtain one at
  https://mozilla.org/MPL/2.0/.
-->

# raptrix-pslf-rs

GE PSLF (`.epc` + `.dyd`) to Raptrix PowerFlow Interchange (`.rpf`). Equipment columns are deterministic. Export timestamps use the wall clock.

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
raptrix-pslf-rs convert --epc my_case.epc --output my_case.rpf --transformer-mode expanded
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
| `--transformer-mode <MODE>` | no | Metadata token `native-3w` (default) or `expanded`. Electrical rows stay two-winding; `transformers_3w` stays empty. |
| `--study-purpose <TEXT>` | no | Metadata override for `metadata.study_purpose`. |
| `--scenario-tag <TAG>` | no | Repeatable metadata override for `metadata.scenario_tags`. |
| `--case-mode <MODE>` | no | Optional override (`flat_start_planning`, `warm_start_planning`, `solved_snapshot`, `hour_ahead_advisory`). |
| `--default-shunt-control-mode <MODE>` | no | Optional override for shunt control mode handoff. |

### view

Pretty-prints table row counts from an existing `.rpf`.

### validate

Parses an `.epc` and reports structural errors. An EPC with no bus records is an error. `--strict` exits non-zero when any error is present.

## Fidelity & Modern Grid Support

- Emits the full set of **18** canonical required RPF **v0.14.5** root tables (zero-row where appropriate), plus `dc_converters` (always emitted, including zero rows; not one of the 18). `dc_lines_2w` stays 15 columns and is empty on this path (`p_setpoint_mw` is rectifier DC power). `branches` is 36 columns; `g_from`, `b_from`, `g_to`, and `b_to` are **null** (`b_shunt` stays total line charging). Nullable `buses.latitude` / `buses.longitude` are always null (no WGS84 in EPC). Facility-membership flags on circuits and transformers are **null**. Switched-shunt control columns are **null** (do not invent MODSW from EPC SVD). `computational_load_profiles` is omitted and `computational_load_mode` is null. `contingencies` is a zero-row stub; `contingency_sequences` is omitted. Root metadata stamps `raptrix.features.dc_converters=true`.
- IBR classification driven from `.dyd` model records (GENROU family + REPC_A / equivalent, matching psse-rs DYR logic where possible).
- `case_fingerprint` is title length plus bus, generator, and load counts. It is not a content hash.
- Full post-write contract validation via `raptrix-cim-arrow::validate_rpf_file`.

See `docs/pslf-mapping.md` for the field-by-field translation rules.

## Local inputs

Licensed decks, if present, live only under `tests/networks/` on a developer machine. That directory is gitignored. Do not commit those files or cite their names, bus ids, or operating points in tracked text.

`tests/golden/` is the local regenerated corpus (`cargo test --test golden_test`). Those `.rpf` files are gitignored.

**Windows note:** If `cargo build` fails with Application Control (error 4551) under OneDrive, build via WSL or set `CARGO_TARGET_DIR` outside the synced folder.

## Building & Releasing

See the scripts/ directory (copied from the psse-rs sibling):

- `scripts/pre-release-check.ps1`
- `scripts/sync-versions.ps1`
- `scripts/package-windows.ps1` / `package-unix.sh`
- `scripts/generate_all_rpfs.sh` — regenerate `tests/golden/*.rpf` from local `tests/networks/` (DYD attached when present)

Standard Rust commands work:

```bash
cargo build --release
cargo test
# Regenerate golden RPFs (requires local licensed EPC/DYD under tests/networks/):
cargo test --test golden_test -- --nocapture
# or: ./scripts/generate_all_rpfs.sh
```

Golden policy (parity with `raptrix-psse-rs`): canonical `tests/golden/<stem>.rpf` includes the matching `.dyd` when available; `_dynamic` / `_static` twins are also written.

## Versioning & Schema Contract

This crate is pinned to **`raptrix-cim-arrow` 0.7.6** (git tag `v0.7.6`, RPF **v0.14.5**). For a sibling `raptrix-cim-rs` checkout, use a **local** `[patch]` in `.cargo/config.toml` (not committed). Every emitted `.rpf` is validated against the locked contract before returning and carries synthesized **`mrid`** identifiers on equipment tables. Facility-membership flags and switched-shunt control columns are emitted null (do not invent BES from kV or MODSW from EPC SVD). Branch-end shunts `g_from` / `b_from` / `g_to` / `b_to` are null. DYD parameters are `p1`..`pN`. Readers accept `v0.14.5` and `0.14.5` only; older files must be re-exported. `v0.15.0` is reserved and is not used. See [MIGRATION.md](MIGRATION.md).

See [raptrix-cim-rs schema-contract](https://github.com/RaptrixPowerFlow/raptrix-cim-rs/blob/main/docs/schema-contract.md) for the full RPF specification.

## License

MPL-2.0. See `LICENSE`.

Copyright (c) 2026 Raptrix PowerFlow.
