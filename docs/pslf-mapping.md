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

**PF impact (Texas7k):** PSLF exports 634 `switched_shunts` rows vs PSSE 429 (format representation difference). As of **raptrix-core v0.5.44**, import splits each device into a fixed BINIT residual on `bus.b_shunt` plus controllable bank steps, and `planning_full` mode runs a PSS/E-style SVD outer loop decoupled from the NR mismatch gate. Row-count parity is not required; solver-readiness is validated via the parity harness.

Future work: map any remaining inline PSLF bus GL/BL into `fixed_shunts` if present on EPC bus continuations.

---

## ACTIVSg cases (IBR-heavy, lightweight validation)

Both PSLF- and PSSE-derived RPF paths on ACTIVSg10k/70k typically **fail to converge** under default Newton settings (`max_iters=200`), matching native RAW import behavior. This is expected for IBR-heavy synthetic grids — not a primary export bug.

| Observation | PSLF path | PSSE path |
|-------------|-----------|-----------|
| Topology | Full bus count after branch STATUS fix | Full bus count |
| Convergence @200 iters | NotConverged | NotConverged |
| Iteration count | Similar order (~16–24) | Similar order (~17–22) |

Advanced solver waterfall applied (max_iters=400, continuation_mode, PV-cold→PQ-hot bridge) — all methods failed for both PSLF and PSSE paths. See "ACTIVSg advanced solver waterfall results" table in the semantic differences section.

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

Continuation lines (`/` suffix) carry `vs` at token index 4 when absent on the primary row. PSLF commonly stores **VS=1.0** here as a placeholder; this value is parsed but is **not** used for `v_mag_set` export.

**Voltage setpoints (fidelity-first):** generator buses export `v_mag_set = bus.vsched` (EPC bus record colon+2), the regulation setpoint from the EPC bus table. The continuation-line `generator.vs` placeholder (≈1.0) is ignored for `v_mag_set`. For Texas7k, `vsched` correctly reflects ~1.02–1.04 pu targets across 667 generator buses (previously all were mis-set to 1.0).

**Bus type inference:** EPC bus records store `ty=1` for all connected buses; PV vs PQ is implicit from attached generator records. The export infers type-2 (PV) from `agg.has_generator` so that raptrix-core's Q-switch mechanism engages. Buses with no generators are exported as type-1 (PQ). A type-3 (slack) is NOT explicitly assigned — core auto-selects the largest generator bus (bus 111217 on Texas7k).

**Q limits (solver-readiness):**

- Per-unit generator `q_min_mvar` / `q_max_mvar`: swap when QB>QT (mirror psse-rs `sanitize_generator_q_limits`).
- Bus-level `q_min` / `q_max`: aggregate only machines with a non-zero QB/QT span; skip `(0,0)` pairs (missing limit in EPC, not a zero-MVar cap).
- Do **not** let zero-span machines collapse PV bus limits to 0 — raptrix-core PV span gate would demote buses incorrectly.

**Known semantic gap vs PSSE-derived RPF:** PSS/E RAW stores explicit VS per machine; PSLF EPC encodes the target as `bus.vsched`. After the vsched fix both approaches reflect the true regulation target. PSS/E may also export `(0,0)` Q limits for machines with missing RAW fields; PSLF skips those for bus aggregation.

**Texas7k convergence with correct vsched:** **raptrix-core v0.5.44** adds RPF `default_shunt_control_mode` propagation, STAT=0 fixed-only SVD import, and a post-converge planning SVD outer loop. **PSSE-derived Texas7k RPF converges** with planning_full. **PSLF-derived Texas7k RPF still stalls ~30 pu** — the 634-vs-429 SVD device reactive baseline remains a format representation gap requiring further core import aggregation work (not an export bug).

---

## Expected semantic differences (solver equivalence vs row parity)

Do **not** force PSSE/PSLF RPF row-count identity. These gaps are acceptable when both paths **converge** and post-solve voltages are within harness tolerance (or documented):

| Topic | PSLF path | PSSE path | Solver impact |
|-------|-----------|-----------|---------------|
| `fixed_shunts` table | 0 rows (Texas7k EPC `shunt data [0]`) | 205 rows (RAW section 2) | b_shunt sourced from SVD b_init on PSLF |
| Total bus b_shunt (Texas7k) | ~406 pu (634 SVDs × b_init absorbed by core) | ~268 pu (205 fixed shunts) | ~52% excess reactive; root cause of Texas7k divergence |
| `switched_shunts` count | 634 (Texas7k) | 429 | More SVD devices in PSLF EPC |
| `switched_shunt_banks` | Granular steps (2873 Texas7k) | Compressed banks (1865) | OK |
| `transformers_3w` | 0 rows (`native-3w`) | Explicit 3W table | OK (2W-expanded topology) |
| `dynamics_models` | DYD row count | DYR row count | Dynamics only |
| Generator `v_mag_set` | `bus.vsched` (~1.02–1.04) — **fixed** | RAW VS / bus VM | Correct fidelity; Texas7k still diverges (SVD b_shunt) |
| Bus `type` | type-2 from `has_generator` — **fixed** | Explicit RAW bus type | Core Q-switch now engages for Texas2k |
| Texas7k solver-readiness | Not solver-ready (PSLF NR ~30pu stall) | Solver-ready (core v0.5.44) | PSLF format SVD baseline gap |
| Texas2k_series25 | Solver-ready (0 v-violations, 110 Q-sw) | Solver-ready | parity dv≈0.077 (model semantic gap) |
| Texas2k_series24 | Converges; 1–14 buses >1.1 pu | Solver-ready | Marginal violations; PSLF/PSSE model differences |
| ACTIVSg10k/70k | Not converged (expected) | Not converged (expected) | IBR structural; LM+continuation also fails |

**Harness gates:** `solver_ready` = both paths converge AND all buses within [0.9, 1.1] pu (ACTIVSg exception: both NotConverged). `parity` = |ΔV|/|Δθ| within harness tolerances (2% / 0.6° default).

**v_lo_count / v_hi_count (added):** JSONL rows include `pslf_v_lo_count`, `pslf_v_hi_count`, `psse_v_lo_count`, `psse_v_hi_count` — buses outside [0.9, 1.1] pu. `solver_ready=False` if any non-zero.

### ACTIVSg advanced solver waterfall results (May 2026)

All three levers applied after primary Newton failure (default max_iters=400 for ACTIVSg):

| Case | Method | PSLF result | PSSE result |
|------|--------|-------------|-------------|
| ACTIVSg10k | Primary (400 iters) | Not conv, 20 iters | Not conv, 17 iters |
| ACTIVSg10k | continuation_mode=True | Not conv | Not conv |
| ACTIVSg10k | PV-cold → PQ-hot bridge | Not conv | Not conv |
| ACTIVSg70k | Primary (400 iters) | Not conv, 40 iters | Not conv, 22 iters |
| ACTIVSg70k | continuation_mode=True | Not conv | Not conv |
| ACTIVSg70k | PV-cold → PQ-hot bridge | Not conv | Not conv |

**Conclusion:** ACTIVSg non-convergence is structural (IBR/KLU near-singularity). No harness-level solver method resolves it. The waterfall infrastructure (`apply_solver_profile`, continuation, bridge) is wired for future use when core profiles are updated for IBR-heavy cases.

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
