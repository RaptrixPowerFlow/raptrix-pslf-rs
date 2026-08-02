<!--
  raptrix-pslf-rs
  Copyright (c) 2026 Raptrix PowerFlow

  This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
  If a copy of the MPL was not distributed with this file, You can obtain one at
  https://mozilla.org/MPL/2.0/.
-->

# GE PSLF → Raptrix PowerFlow Interchange — Field Mapping

**raptrix-pslf-rs**

This document provides the field-by-field rules for translating GE PSLF EPC (power flow) and DYD (dynamics) records into the Raptrix PowerFlow Interchange (`.rpf` / RPF **v0.13.0**) Apache Arrow schema.

**Fidelity policy**: numeric fields are written exactly as they appear in the source EPC file unless an explicit normalisation rule is documented below. No value clamping, substitution, or scaling is applied at parse time except where required to match the RPF schema units (e.g. MVA → per-unit on SBASE). Validation and singularity handling are the responsibility of the downstream solver.

---

## Version compatibility

- **RPF contract**: **v0.13.0** emit only (`raptrix-cim-arrow` 0.6.0). Pre-0.13 `.rpf` files must be re-exported (clean cut; no dual-read).
- Equipment tables include nullable trailing **`mrid`** on new exports; `buses.latitude` / `buses.longitude` are null (no WGS84 in EPC).
- Optional tables (`remedial_action_schemes`, `contingency_island_analysis`, `scenario_context`, computational load profiles) are not emitted on the standard PSLF export path.
- Targets GE PSLF EPC files compatible with the provided reference cases (Texas synthetic grids).
- DYD model records for IBR classification and `dynamics_models` table (GENROU/REPC family and equivalents — aligned with psse-rs DYR handling). `classical_params` is present and null (DYD numerics are positional).

## 3-Winding Transformers

Tertiary winding data appears in the `transformer data` section (columns `ts_r`, `ts_x`, `tert*`, etc.). The parser inspects these records to decide native 3W vs. expanded representation (controlled by `--transformer-mode`, default `native-3w` for CLI parity with psse-rs).

(See implementation notes in the source for exact heuristics once the parser is complete.)

## Export metadata (aligned with psse-rs)

- **`source_format`**: `pslf_epc`; **`source_identity_scheme`**: `dense_bus_id`; **`source_format_version`**: null.
- **`rpf.identity.model`**: `hybrid_solver_flat_v1`.
- **`case_mode`**: Auto-detected from EPC bus `volt` / `angle` — `flat_start_planning` when all buses are at 1.0 pu / 0°, otherwise `warm_start_planning`. Override with `--case-mode`.
- **`default_shunt_control_mode`**: Set to `planning_full` for planning case modes (same rule as psse-rs). Override with `--default-shunt-control-mode`.

---

## Fixed shunts (table row-count gap)

GE PSLF often stores fixed shunt admittance **inline on bus records** or in vendor-specific sections, while PSS/E uses an explicit `fixed shunt` table in RAW.

| Case | PSLF EPC | PSSE RAW | RPF export today |
|------|----------|----------|------------------|
| large benchmark case | `shunt data [0]` | 205 fixed shunts | PSLF: 0 `fixed_shunts` rows; PSSE: 205 rows |

**PF impact (large benchmark case):** PSLF exports 634 `switched_shunts` rows vs PSSE 429 (format representation difference). As of **raptrix-core v0.5.64**, import splits each device into a fixed BINIT residual on `bus.b_shunt` plus controllable bank steps, and `planning_full` mode runs a PSS/E-style SVD outer loop decoupled from the NR mismatch gate. Row-count parity is not required; solver-readiness is validated via the parity harness.

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

PSLF expands to granular `switched_shunt_banks` rows (e.g. 2873 steps on large benchmark case). PSS/E often compresses banks (1865 rows). Bus-level `switched_shunts` counts match when the physical device set aligns.

**series24 case4/case6 (157 vs 153):** EPC `svd data` contains **157** primary records per case; PSLF export is faithful (157 rows). Matching PSSE RAW exports **153** rows — four fewer devices in the RAW source, not dropped EPC records. **No parser change**; document as acceptable format gap.

---

## Branches and transformers (impedance units)

EPC `branch data` and `transformer data` store **R, X, B in per-unit on SBASE** (PSS/E RAW convention). The RPF wire contract matches PSS/E: write those **system-base pu** values directly into `branches.r` / `branches.x` / `branches.b_shunt` and the transformer equivalents (same as `raptrix-psse-rs`).

Do **not** scale by \(Z_\mathrm{base}=V^2/S_\mathrm{base}\). An older PSLF-only physical-ohm export path inflated series reactance by ~\(115^2/100=132.25\) on 115 kV decks and prevented NR convergence despite correct bus types.

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

**Voltage setpoints (fidelity-first):** generator buses export `v_mag_set = bus.vsched` (EPC bus record colon+2), the regulation setpoint from the EPC bus table. The continuation-line `generator.vs` placeholder (≈1.0) is ignored for `v_mag_set`. For large benchmark case, `vsched` correctly reflects ~1.02–1.04 pu targets across 667 generator buses (previously all were mis-set to 1.0).

**Bus type mapping (PSLF `ty` → RPF dictionary, RAW IDE–aligned):**

| PSLF `ty` | Meaning | RPF `buses.type` |
|-----------|---------|------------------|
| `0` | swing | `Slack` (always — never demote / override) |
| `1` | load | `PQ` |
| `2` | generator | `PV` **only if** an online machine (`status != 0`) is present; else `PQ` |
| `3` | (rare / PSSE-like) | `Slack` |

Texas2k series24 EPCs store a real swing bus (`ty=0`, bus **7389**) and keep offline plant buses as `ty=2` while the twin RAW uses `IDE=1`. The online-machine gate demotes those offline plants to `PQ` so type histograms match RAW. Area `swing_bus` is often `0` and is **not** used for typing.

**Remote regulation:** `generators.controlled_bus_id` is null for local regulation (`ireg == 0` or `ireg == bus`); otherwise the remote dense bus id.

**Q limits (solver-readiness):**

- Per-unit generator `q_min_mvar` / `q_max_mvar`: swap when QB>QT (mirror psse-rs `sanitize_generator_q_limits`).
- Bus-level `q_min` / `q_max`: aggregate only machines with a non-zero QB/QT span; skip `(0,0)` pairs (missing limit in EPC, not a zero-MVar cap).
- Do **not** let zero-span machines collapse PV bus limits to 0 — raptrix-core PV span gate would demote buses incorrectly.

**Known semantic gap vs PSSE-derived RPF:** PSS/E RAW stores explicit VS per machine; PSLF EPC encodes the target as `bus.vsched`. After the vsched fix both approaches reflect the true regulation target. PSS/E may also export `(0,0)` Q limits for machines with missing RAW fields; PSLF skips those for bus aggregation.

**large benchmark case convergence with correct vsched:** **raptrix-core v0.5.64** adds RPF `default_shunt_control_mode` propagation, STAT=0 fixed-only SVD import, and a post-converge planning SVD outer loop. **PSSE-derived large benchmark case RPF converges** with planning_full. **PSLF-derived large benchmark case RPF still stalls ~30 pu** — the 634-vs-429 SVD device reactive baseline remains a format representation gap requiring further core import aggregation work (not an export bug).

---

## Expected semantic differences (solver equivalence vs row parity)

Do **not** force PSSE/PSLF RPF row-count identity. These gaps are acceptable when both paths **converge** and post-solve voltages are within harness tolerance (or documented):

| Topic | PSLF path | PSSE path | Solver impact |
|-------|-----------|-----------|---------------|
| `fixed_shunts` table | 0 rows (large benchmark case EPC `shunt data [0]`) | 205 rows (RAW section 2) | b_shunt sourced from SVD b_init on PSLF |
| Total bus b_shunt (large benchmark case) | ~406 pu (634 SVDs × b_init absorbed by core) | ~268 pu (205 fixed shunts) | ~52% excess reactive; root cause of large benchmark case divergence |
| `switched_shunts` count | 634 (large benchmark case) | 429 | More SVD devices in PSLF EPC |
| `switched_shunt_banks` | Granular steps (2873 large benchmark case) | Compressed banks (1865) | OK |
| `transformers_3w` | 0 rows (`native-3w`) | Explicit 3W table | OK (2W-expanded topology) |
| `dynamics_models` | DYD row count | DYR row count | Dynamics only |
| Generator `v_mag_set` | `bus.vsched` (~1.02–1.04) — **fixed** | RAW VS / bus VM | Correct fidelity; large benchmark case still diverges (SVD b_shunt) |
| Bus `type` | PSLF `ty` + online-machine gate (Slack from `ty=0`) | Explicit RAW IDE tokens | Histograms match RAW on series24 |
| Branch table rows | Keep OOS branches (`status=false`) | Keep OOS (psse-rs); core native RAW may drop OOS | Compare in-service counts, not raw `get_branches()` |
| large benchmark case solver-readiness | Not solver-ready (PSLF NR ~30pu stall) | Solver-ready (core v0.5.64) | PSLF format SVD baseline gap |
| Texas2k_series25 | Solver-ready (0 v-violations, 110 Q-sw) | Solver-ready | parity dv≈0.077 (model semantic gap) |
| Texas2k_series24 | Slack/type aligned with RAW; cold NR expected | Solver-ready | Prior auto-slack mismatch was export bug |
| ACTIVSg10k/70k | Not converged (expected) | Not converged (expected) | IBR structural; LM+continuation also fails |
| Eastern / Midwest24k | **Not in this corpus** (no EPC) | Available under psse-rs `tests/data/external/` | psse-rs ownership |

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

## Table-by-Table Mapping

Mirrors the style and depth of `docs/psse-mapping.md` in the PSS/E sibling crate. Core export builders live in `src/export.rs`.

| Table | Status |
|-------|--------|
| `metadata` | Implemented — v0.13.0 provenance, case_mode, modern-grid flags, study/scenario metadata |
| `buses` | Implemented — dictionary type tokens, vsched PV setpoints, Q aggregation, bus_uuid |
| `generators` | Implemented — nullable controlled_bus_id, IBR subtype from DYD, params map, Q sanitization |
| `loads` | Implemented — constant-PQ; ZIP columns null; trailing `mrid` null |
| `branches` | Implemented — system-base pu R/X/B (parity with psse-rs), FACTS columns null |
| `transformers_2w` / `transformers_3w` | Implemented — native-3w mode (3W table often zero-row) |
| `switched_shunts` + `switched_shunt_banks` | Implemented — granular PSLF SVD steps; trailing `mrid` null |
| `fixed_shunts` | Implemented — zero-row when EPC has no explicit shunt table; trailing `mrid` null |
| `dynamics_models` | Implemented when `.dyd` supplied; `classical_params` null |
| `areas`, `zones`, `owners` | Implemented |

Optional tables (`remedial_action_schemes`, `contingency_island_analysis`, `scenario_context`, computational load profiles) are not emitted on the standard PSLF path.
