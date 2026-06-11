# Changelog — raptrix-pslf-rs

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

_No user-facing changes yet._

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
