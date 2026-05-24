<!--
  raptrix-pslf-rs
  Copyright (c) 2026 Raptrix PowerFlow

  This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
  If a copy of the MPL was not distributed with this file, You can obtain one at
  https://mozilla.org/MPL/2.0/.
-->

# GE PSLF → Raptrix PowerFlow Interchange — Field Mapping

**raptrix-pslf-rs**

This document provides the field-by-field rules for translating GE PSLF EPC (power flow) and DYD (dynamics) records into the Raptrix PowerFlow Interchange (`.rpf` / RPF **v0.10.0**) Apache Arrow schema.

**Fidelity policy**: numeric fields are written exactly as they appear in the source EPC file unless an explicit normalisation rule is documented below. No value clamping, substitution, or scaling is applied at parse time except where required to match the RPF schema units (e.g. MVA → per-unit on SBASE). Validation and singularity handling are the responsibility of the downstream solver.

---

## Version compatibility

- Targets GE PSLF EPC files compatible with the provided reference cases (Texas synthetic grids).
- DYD model records for IBR classification and `dynamics_models` table (GENROU/REPC family and equivalents — aligned with psse-rs DYR handling).

## 3-Winding Transformers

Tertiary winding data appears in the `transformer data` section (columns `ts_r`, `ts_x`, `tert*`, etc.). The parser inspects these records to decide native 3W vs. expanded representation (controlled by `--transformer-mode`, default `native-3w` for CLI parity with psse-rs).

(See implementation notes in the source for exact heuristics once the parser is complete.)

## Export metadata (aligned with psse-rs)

- **`case_mode`**: Auto-detected from EPC bus `volt` / `angle` — `flat_start_planning` when all buses are at 1.0 pu / 0°, otherwise `warm_start_planning`. Override with `--case-mode`.
- **`default_shunt_control_mode`**: Set to `planning_full` for planning case modes (same rule as psse-rs). Override with `--default-shunt-control-mode`.

---

## Fixed shunts (table row-count gap)

GE PSLF often stores fixed shunt admittance **inline on bus records** or in vendor-specific sections, while PSS/E uses an explicit `fixed shunt` table in RAW.

| Case | PSLF EPC | PSSE RAW | RPF export today |
|------|----------|----------|------------------|
| Texas7k | `shunt data [0]` | 205 fixed shunts | PSLF: 0 `fixed_shunts` rows; PSSE: 205 rows |

**PF impact (Texas7k, fixed):** Bus-level `b_shunt` after import now matches PSSE (~267.7 pu) via **SVD `b_init`** mapping (see below). The missing `fixed_shunts` table rows are a structural diff only for Texas7k; they no longer block convergence.

Future work: map any remaining inline PSLF bus GL/BL (if present on EPC bus continuation) into `fixed_shunts` for full table parity.

---

## Switched shunts (SVD)

PSLF `svd data` fields are **per-unit on system base** (same as applied by raptrix-core on import), not MVar.

| Field | EPC token (after `:`) | RPF column | Notes |
|-------|----------------------|------------|-------|
| `b_init` | `+9` | `switched_shunts.b_init_pu` | Do **not** divide by `base_mva` on export |
| `vband` | `+12` | `v_low` / `v_high` | Voltage band limits |
| Step `b` | step list | `b_steps` / banks | Positive steps only; stored in pu |

PSLF expands to granular `switched_shunt_banks` rows (e.g. 2873 steps on Texas7k). PSS/E often compresses banks (1865 rows). Bus-level `switched_shunts` counts match when the physical device set aligns.

---

## Branches and transformers (impedance units)

EPC `branch data` and `transformer data` store **R, X, B in per-unit on SBASE** (PSS/E RAW convention). raptrix-core treats non-PSS/E RPF branch/transformer rows with `from_nominal_kv > 0` as **physical units** (Ω, S) and converts with `Z_base = V²/S_base`.

**Export rule (`export.rs`):** write physical values into the RPF so import recovers pu:

- `r_export = r_pu × Z_base`
- `x_export = x_pu × Z_base`
- `b_export = b_pu / Z_base`

Use `from_bus` nominal kV for lines; `max(from_kv, to_kv)` for transformers.

**Transformer parse (`parser.rs`):**

| Data | Source |
|------|--------|
| `ps_r`, `ps_x` | Last 7 numerics on header line: `tbase ps_r ps_x pt_r pt_x ts_r ts_x` |
| `from_kv`, `to_kv` | Continuation line 1: first two kV fields (not tap) |
| `tap` (WINDV) | Continuation line 2: second numeric when ≤ 5.0 (e.g. `1 1.000 ...`) |
| `rate_a/b/c` | Continuation line 1, indices 6–8 |
| `nominal_tap_ratio` | Export: `from_kv / to_kv`; `tap_ratio` = WINDV (default 1.0) |

---

## Table-by-Table Mapping (Work in Progress)

This section will be populated as each `build_*_batch` function is implemented. It will mirror the style and depth of `docs/psse-mapping.md` in the sibling crate.

- `buses`
- `generators` (including `is_ibr` / `ibr_subtype` from DYD)
- `loads`
- `branches`
- `transformers_2w` / `transformers_3w`
- `switched_shunts` + `switched_shunt_banks`
- `fixed_shunts`
- `dynamics_models`
- `metadata` (case_mode, fingerprints, study_purpose, scenario_tags, etc.)
- ... (all 18 canonical tables)

---

**Status**: Skeleton created during scaffold phase. Real content will be written during the export builder implementation phases.
