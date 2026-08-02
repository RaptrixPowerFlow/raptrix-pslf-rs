# Changelog — raptrix-pslf-rs

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Bus type fidelity (Texas2k series24 / RAW IDE parity)

- Map PSLF `ty=0` → explicit `Slack` (e.g. bus **7389** on series24); never leave zero Slack for core auto-pick.
- Demote offline plant buses (`ty=2` without an online machine) to `PQ` so PV histograms match twin RAW IDE.
- Document branch OOS row retention vs core native RAW drop; Eastern/Midwest24k remain psse-rs-only (no EPC).
- Canonical local corpus remains `tests/golden/` (not stale `tests/compare/`).

### Branch / transformer impedance (system pu)

- Stop writing branch/transformer `r`/`x`/`b` as physical ohms via \(Z_\mathrm{base}\). Emit **system-base pu** like `raptrix-psse-rs` (fixes series24 non-convergence from ~132.25× reactance inflation).
- `transformers_2w.phase_shift` exported in **degrees** per the RPF field guide (was incorrectly converted to radians).

### Operating-state replay

- Export `buses.v_mag_set` from EPC `bus.volt` (operating-point seed), not `bus.vsched`.
- Preserve `bus.vsched` as the AVR target in `generators.params["vs"]`, replacing the commonly placeholder machine `vs`.
- Keep inactive switched-shunt rows and step capability, but force `current_step=0` and `b_init_pu=0` so offline SVD devices do not inject reactive power.

---

## [0.6.0] - 2026-07-30

### RPF **v0.13.0** (`raptrix-cim-arrow` **0.6.0**)

- **Emit RPF v0.13.0 only** (clean cut; re-export required for all pre-0.13 `.rpf` files).
- Provenance: `source_format=pslf_epc`, `source_format_version` null (no EPC rev stamp), `source_identity_scheme=dense_bus_id`.
- Bus types as dictionary tokens `PQ`/`PV`/`Slack`; `controlled_bus_id` null for local IREG; native UTC timestamps.
- Optional `mrid` on loads/shunts (null from PSLF); `classical_params` column on dynamics (null — DYD params are positional).
- Root metadata stamps `rpf.identity.model=hybrid_solver_flat_v1`.
- **`baseline_source_case_id`** replaces any prior `original_sentinel_case_id` wire name (null on standard PSLF planning exports).
- **Dependency**: `raptrix-cim-arrow` **0.6.0** / git tag **`v0.6.0`**.

---

## [0.5.7] - 2026-07-16

### RPF **v0.12.5** (`raptrix-cim-arrow` **0.5.7**)

- **Emit RPF v0.12.5**: every `.rpf` carries `raptrix.version` / contract **v0.12.5**.
- **Nullable trailing `buses.latitude` / `buses.longitude`**: emitted as null (PSLF EPC has no standard WGS84 bus coordinates). Electrical planning payload unchanged from 0.5.6 aside from the contract stamp.
- **No re-export required** for existing **v0.12.1+** `.rpf` files.
- **Dependency**: `raptrix-cim-arrow` **0.5.7** / git tag **`v0.5.7`**.

---

## [0.5.6] - 2026-07-02

### RPF **v0.12.4** (`raptrix-cim-arrow` **0.5.6**)

- **Emit RPF v0.12.4**: version stamp and reader pin only on the planning export path; nullable v0.12.3/v0.12.4 metadata columns remain null and optional solved-state tables zero-row.
- **No re-export required** for existing **v0.12.1+** `.rpf` files.
- **Dependency**: `raptrix-cim-arrow` **0.5.6** / git tag **`v0.5.6`**.

---

## [0.5.4] - 2026-06-15

### RPF **v0.12.2** (`raptrix-cim-arrow` **0.5.4**)

- **Emit v0.12.2**: every `.rpf` from this crate carries `raptrix.version` / contract **v0.12.2** (via `raptrix-cim-arrow::SCHEMA_VERSION`) and file metadata **`rpf.mrid_support = v1`**.
- **Additive nullable `mrid` column** on `branches`, `generators`, `transformers_2w`, and `transformers_3w`. PSLF exports populate deterministic vendor-path identifiers (`BR_`, `GEN_`, `XF2_`, `XF3_`, star-leg `{parent}_H/_M/_L` suffixes) on all new writes.
- **No re-export required** for existing v0.12.1 files — readers pad missing trailing columns as null. Downstream tools should prefer `mrid` for equipment_id mapping.
- **Dependency**: `raptrix-cim-arrow` **0.5.4** / git **`c45256e`**.

---

## [0.5.3] - 2026-06-10

### RPF **v0.12.1** (`raptrix-cim-arrow` **0.5.3**)

- **Emit-only v0.12.1**: every `.rpf` from this crate carries `raptrix.version` / contract **v0.12.1** (via `raptrix-cim-arrow::SCHEMA_VERSION`). Optional `remedial_action_schemes` / `contingency_island_analysis` root tables are not emitted on the standard PSLF path.
- **Export parity** with `raptrix-psse-rs` v0.12.1: metadata modern-grid flags, generator `params` maps, branch/transformer nominal kV resolution (opposite-bus fallback), `generators.controlled_bus_id` IREG semantics, full `transformers_3w` schema columns, root metadata keys (`rpf.case_mode`, `rpf.default_shunt_control_mode`, `rpf.loads.zip_fidelity_presence`, etc.), and pre-write export invariant checks.
- **`SUPPORTED_RPF_VERSIONS`** in the linked crate accepts **only** **v0.12.1** / **0.12.1** — re-export all cached `.rpf` files.
- **Dependency**: `raptrix-cim-arrow` **0.5.3** / git **`298f9958cb9a551e273257f045bcadc1c72cf7bb`**.
- **CI**: GitHub workflows for fmt/clippy/test, version consistency, and public-safety hygiene (mirrors `raptrix-psse-rs`).
- **Release**: GitHub Release workflow builds Windows, Linux, and macOS (Apple Silicon) binaries when version tag `v0.5.3` is pushed.

---

## [0.1.0] — Initial scaffold

- Project scaffold mirroring `raptrix-psse-rs` architecture.
- EPC + DYD parser and canonical RPF export for Texas reference cases.
- Cross-tool row-count parity tests with `raptrix-psse-rs` where cases overlap.
