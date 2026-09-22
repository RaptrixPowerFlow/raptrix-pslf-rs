<!--
  raptrix-pslf-rs
  Copyright (c) 2026 Raptrix PowerFlow

  This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
  If a copy of the MPL was not distributed with this file, You can obtain one at
  https://mozilla.org/MPL/2.0/.
-->

# GE PSLF → Raptrix PowerFlow Interchange — Field Mapping

**raptrix-pslf-rs**

This document provides the field-by-field rules for translating GE PSLF EPC (power flow) and DYD (dynamics) records into the Raptrix PowerFlow Interchange (`.rpf` / RPF **v0.14.4**) Apache Arrow schema.

**Fidelity policy**: numeric fields are written exactly as they appear in the source EPC file unless an explicit normalisation rule is documented below. No value clamping, substitution, or scaling is applied at parse time except where required to match the RPF schema units (e.g. MVA → per-unit on SBASE). Validation and singularity handling are the responsibility of the downstream solver.

---

## Version compatibility

- **RPF contract**: **v0.14.4** emit (`raptrix-cim-arrow` 0.7.4). Readers accept **`v0.14.4`** and **`0.14.4`** only. v0.14.3 and every older stamp are rejected. Re-export cached `.rpf` files. `v0.15.0` is reserved and is not used. Optional CLP columns are not authored on this path (`computational_load_profiles` omitted, `computational_load_mode` null). `shunt_control_mode` / `regulated_bus_id` are **null** — do not invent MODSW from EPC SVD.
- Trailing nullable **`is_secured` / `is_bes` / `is_bps` / `is_bptf`** on `branches`, `transformers_2w`, `transformers_3w`, and `multi_section_lines` are **null**. Do not invent BES from kV.
- `branches` is 36 columns. Trailing **`g_from` / `b_from` / `g_to` / `b_to`** are **null** (this source has R, X, and B only). `b_shunt` stays total line charging. Bus `g_shunt` / `b_shunt` aggregation is unchanged.
- Equipment tables include nullable trailing **`mrid`** on new exports; `buses.latitude` / `buses.longitude` are null (no WGS84 in EPC).
- Optional tables (`remedial_action_schemes`, `contingency_island_analysis`, `contingency_sequences`, `scenario_context`, computational load profiles) are not emitted on the standard PSLF export path. `dc_converters` is always emitted, including zero rows, and is not one of the 18 canonical tables (`raptrix.features.dc_converters=true`). `dc_lines_2w` stays the empty 15-column table (`p_setpoint_mw` is rectifier DC power). `contingencies` is a zero-row stub (`tpl_category` / `reserved` null).
- DYD records feed IBR classification and `dynamics_models`. `classical_params` is present and null (DYD numerics stay positional).

## Transformers

Every transformer header is stored as a two-winding record. Tertiary windings are not modeled, so `transformers_3w` is empty. `--transformer-mode` sets `rpf.transformer_representation_mode` (`native_3w` or `expanded`) and does not change electrical columns.

## Export metadata

- **`source_format`**: `pslf_epc`; **`source_identity_scheme`**: `dense_bus_id`; **`source_format_version`**: null.
- **`rpf.identity.model`**: `hybrid_solver_flat_v1`.
- **`case_mode`**: Auto-detected from EPC bus `volt` / `angle` — `flat_start_planning` when all buses are at 1.0 pu / 0°, otherwise `warm_start_planning`. Override with `--case-mode`.
- **`default_shunt_control_mode`**: `planning_full` for planning case modes. Override with `--default-shunt-control-mode`.

---

## Fixed shunts

`fixed_shunts` rows come only from an explicit `shunt data` section. When that section is absent or empty, the table is zero-row. Inline bus shunt, when present on a bus continuation, is not copied into `fixed_shunts` by this converter.

---

## Switched shunts (SVD)

PSLF `svd data` fields are **per-unit on system base**, not Mvar.

| Field | EPC token (after `:`) | RPF column | Notes |
|-------|----------------------|------------|-------|
| `b_init` | `+9` | `switched_shunts.b_init_pu` | Do **not** divide by `base_mva`; force `0` when `status=0` |
| `vband` | `+12` | `v_low` / `v_high` | Voltage band limits |
| Step `b` | continuation `(N, B)` pairs | `b_steps` / banks | Positive `B` only at export; stored in pu |

A continuation line is a run of `(N, B)` pairs (`N` integer step count, `B` per-step susceptance with a decimal point), then an integer tail that is not a bank. The parser keeps every leading pair, including negative `B`. Export drops non-positive steps.

An out-of-service SVD stays in the table with `status=false`, `current_step=0`, and `b_init_pu=0`. Its positive steps remain in the bank table. `shunt_control_mode` and `regulated_bus_id` stay null.

---

## Branches and transformers (impedance units)

EPC `branch data` and `transformer data` store **R, X, and B in per-unit on SBASE**. Write those system-base pu values into `branches.r`, `branches.x`, `branches.b_shunt`, and the transformer equivalents.

Do **not** scale by \(Z_\mathrm{base}=V^2/S_\mathrm{base}\). `branches.g_from`, `b_from`, `g_to`, and `b_to` stay null. `b_shunt` stays total line charging.

`from_nominal_kv` / `to_nominal_kv` remain required metadata for topology/GIS consumers; they are not a cue to rewrite impedance into ohms.

**Transformer parse (`parser.rs`):**

| Data | Source |
|------|--------|
| `ps_r`, `ps_x` | Last 7 numerics on header line: `tbase ps_r ps_x pt_r pt_x ts_r ts_x` |
| `from_kv`, `to_kv` | Continuation line 1: first two kV fields (not tap) |
| `tap` (WINDV) | Continuation line 2: second numeric when ≤ 5.0 (e.g. `1 1.000 ...`) |
| `rate_a/b/c` | Continuation line 1, indices 6–8 |
| `nominal_tap_ratio` | Export: `from_kv / to_kv`; `tap_ratio` = WINDV (default 1.0) |

---

## Generators

EPC `generator data` primary line (after `:`) token indices relative to the colon:

| Field | Token offset | Meaning |
|-------|--------------|---------|
| `status` | `+1` | In service when non-zero |
| `ireg` | `+2` | Remote regulation bus; `0` or the machine bus means local |
| `pg` | `+9` | Scheduled MW |
| `pt` | `+10` | Max MW |
| `pb` | `+11` | Min MW |
| `qg` | `+12` | Scheduled Mvar |
| `qt` | `+13` | Max Mvar |
| `qb` | `+14` | Min Mvar |
| `mbase` | `+15` | Machine base MVA |

Continuation lines (`/` suffix) carry `vs` at token index 4 when absent on the primary row. PSLF commonly stores **VS=1.0** here as a placeholder.

**Voltage seed vs. control target (fidelity-first):**

- `buses.v_mag_set = bus.volt`: the authored operating-point seed used to replay the EPC state.
- `generators.params["vs"] = bus.vsched`: the AVR voltage-control target. This overrides the machine continuation-line placeholder when a bus record is available.

These values are intentionally distinct. Writing `bus.vsched` into `v_mag_set` perturbs the initial network state and can trigger premature Q-limit switching.

**Bus type mapping (`ty` → RPF dictionary):**

| PSLF `ty` | Meaning | RPF `buses.type` |
|-----------|---------|------------------|
| `0` | swing | `Slack` (always — never demote / override) |
| `1` | load | `PQ` |
| `2` | generator | `PV` **only if** an online machine (`status != 0`) is present; else `PQ` |
| `3` | swing | `Slack` |

`ty=0` is always `Slack`. A `ty=2` bus with no in-service machine is `PQ`. Area `swing_bus` is not used for typing.

**Remote regulation:** `generators.controlled_bus_id` is null for local regulation (`ireg == 0` or `ireg == bus`); otherwise the remote dense bus id.

**Q limits (solver-readiness):**

- Generator `q_min_mvar` / `q_max_mvar`: swap when the min is greater than the max.
- Bus-level `q_min` / `q_max`: aggregate only machines with a non-zero QB/QT span. A `(0, 0)` pair is a missing limit, not a zero-Mvar cap.

---

## Table-by-Table Mapping

Builders live in `src/export.rs`.

| Table | Status |
|-------|--------|
| `metadata` | Implemented — v0.13.0 provenance, case_mode, modern-grid flags, study/scenario metadata |
| `buses` | Implemented — dictionary type tokens, `bus.volt` operating seed, Q aggregation, bus_uuid |
| `generators` | Implemented — nullable controlled_bus_id, IBR subtype from DYD, params map, Q sanitization |
| `loads` | Implemented — constant-PQ; ZIP columns null; trailing `mrid` null |
| `branches` | Implemented — system-base pu R/X/B, FACTS columns null. Width 36. Trailing `g_from` / `b_from` / `g_to` / `b_to` are null. `b_shunt` stays total line charging. |
| `transformers_2w` / `transformers_3w` | Two-winding records only. `transformers_3w` is empty. `phase_shift` is degrees. |
| `switched_shunts` + `switched_shunt_banks` | Implemented — granular PSLF SVD steps; trailing `mrid` null |
| `fixed_shunts` | Implemented — zero-row when EPC has no explicit shunt table; trailing `mrid` null |
| `dynamics_models` | Implemented when `.dyd` supplied; `classical_params` null |
| `areas`, `zones`, `owners` | Implemented |
| `contingencies` | Zero-row stub — 10-column v0.14.1 schema; `tpl_category` / `reserved` null |
| `dc_lines_2w` | Empty 15-column table. `p_setpoint_mw` is rectifier DC power. Two-terminal DC sections are not ingested. |
| `dc_converters` | Always emitted, including zero rows. Not one of the 18 canonical tables. |

Optional tables (`remedial_action_schemes`, `contingency_island_analysis`, `contingency_sequences`, `scenario_context`, computational load profiles) are not emitted on the standard PSLF path. `dc_converters` is emitted with zero rows.
