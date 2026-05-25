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

**PF impact (Texas7k):** After raptrix-core RPF import, summed bus `b_shunt` differs by path (PSLF ~406 pu vs PSSE ~268 pu on Texas7k) because PSLF exports more `switched_shunts` device rows (634 vs 429) whose `b_init` rolls into bus shunt at import. Both paths converge; this is a **structural/device-count** difference, not a missing fixed-shunt table alone.

Future work: map any remaining inline PSLF bus GL/BL (if present on EPC bus continuation) into `fixed_shunts` for full table parity.

---

## ACTIVSg cases (IBR-heavy, lightweight validation)

Both PSLF- and PSSE-derived RPF paths on ACTIVSg10k/70k typically **fail to converge** under default Newton settings (`max_iters=200`), matching native RAW import behavior. This is expected for IBR-heavy synthetic grids — not a primary export bug.

| Observation | PSLF path | PSSE path |
|-------------|-----------|-----------|
| Topology | Full bus count after branch STATUS fix | Full bus count |
| Convergence @200 iters | NotConverged | NotConverged |
| Iteration count | Similar order (~16–24) | Similar order (~17–22) |

Optional one-off: raise `max_iters` to 400 — both paths still non-converged at similar iteration counts (10k: 16/17 iters, 70k: 24/22 iters); document outcome in harness JSONL rather than blocking Texas parity work.

**Structural gap:** PSLF `native-3w` export emits 0 `transformers_3w` rows; PSSE keeps explicit 3W table rows. Solver uses 2W-expanded topology on the PSLF path.

---

## Switched shunts (SVD)

PSLF `svd data` fields are **per-unit on system base** (same as applied by raptrix-core on import), not MVar.

| Field | EPC token (after `:`) | RPF column | Notes |
|-------|----------------------|------------|-------|
| `b_init` | `+9` | `switched_shunts.b_init_pu` | Do **not** divide by `base_mva` on export |
| `vband` | `+12` | `v_low` / `v_high` | Voltage band limits |
| Step `b` | step list | `b_steps` / banks | Positive steps only; stored in pu |

PSLF expands to granular `switched_shunt_banks` rows (e.g. 2873 steps on Texas7k). PSS/E often compresses banks (1865 rows). Bus-level `switched_shunts` counts match when the physical device set aligns.

**series24 case4/case6 (157 vs 153):** EPC `svd data` contains **157** primary records per case; PSLF export is faithful (157 rows). Matching PSSE RAW exports **153** rows — four fewer devices in the RAW source, not dropped EPC records. **No parser change**; document as acceptable format gap.

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

## Generators

EPC `generator data` primary line (after `:`) token indices relative to the colon:

| Field | Token offset | PSSE RAW analogue |
|-------|--------------|-------------------|
| `status` | `+1` | STAT |
| `ireg` | `+2` | IREG (remote regulation bus) |
| `pg` | `+9` | PG |
| `pt` | `+10` | PT |
| `pb` | `+11` | PB |
| `qg` | `+12` | QG |
| `qt` | `+13` | QT (qmax in EPC header) |
| `qb` | `+14` | QB (qmin in EPC header) |
| `mbase` | `+15` | MBASE |

Continuation lines (`/` suffix) carry `vs` at token index 4 when absent on the primary row. PSLF commonly stores **VS=1.0** here as a placeholder.

**Voltage setpoints (solver-readiness):** when `generator.vs > 0`, export applies it as `v_mag_set` on generator buses (typically 1.0 pu from continuation). EPC bus `volt` warm-start values are overridden on gen buses. Texas7k **does not converge** in raptrix-core when gen-bus `v_mag_set` follows EPC `volt` instead (~30 pu mismatch vs ~2e-9 with VS=1.0). Document as a PSLF↔core semantic gap until core accepts EPC VM as the PV target.

**Q limits (solver-readiness):**

- Per-unit generator `q_min_mvar` / `q_max_mvar`: swap when QB>QT (mirror psse-rs `sanitize_generator_q_limits`).
- Bus-level `q_min` / `q_max`: aggregate only machines with a non-zero QB/QT span; skip `(0,0)` pairs (missing limit in EPC, not a zero-MVar cap).
- Do **not** let zero-span machines collapse PV bus limits to 0 — raptrix-core PV span gate would demote buses incorrectly.

**Known semantic gap vs PSSE-derived RPF:** PSS/E uses RAW `VS` on generator buses; PSLF export uses continuation VS≈1.0 for convergence. PSS/E may also export `(0,0)` Q limits for machines with missing RAW fields; PSLF skips those for bus aggregation.

---

## Expected semantic differences (solver equivalence vs row parity)

Do **not** force PSSE/PSLF RPF row-count identity. These gaps are acceptable when both paths **converge** and post-solve voltages are within harness tolerance (or documented):

| Topic | PSLF path | PSSE path | Solver impact |
|-------|-----------|-----------|---------------|
| `fixed_shunts` table | Often 0 rows (inline/EPC) | Explicit RAW table | OK if bus shunt effect similar |
| `switched_shunts` count | One EPC record per device | One RAW record per device (may differ) | OK if both converge |
| `switched_shunt_banks` | Granular steps | Compressed banks | OK |
| `transformers_3w` | 0 rows (`native-3w`) | Explicit 3W table | OK (2W-expanded topology) |
| `dynamics_models` | DYD row count | DYR row count | Dynamics only |
| Generator `v_mag_set` | Continuation VS≈1.0 on gen buses | RAW VS / bus VM | Voltage parity gap (Texas7k ~45% \|ΔV\| vs full PSSE) |
| Texas7k \|ΔV\| | — | — | Both converge; parity FAIL is documented, not blocking |

Harness gates: **`solver_ready`** = both paths converge (ACTIVSg: both NotConverged is documented exception). **`parity`** = \|ΔV\|/\|Δθ\| vs tolerances (2% / 0.6° default).

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
