# Migration — raptrix-pslf-rs

## v0.6.0 / RPF v0.13.0 (breaking clean cut — re-export required)

`raptrix-pslf-rs` **0.6.0** emits RPF **v0.13.0** only.

### What changed

- Dropped required `psse_version`; provenance is `source_format=pslf_epc`, nullable `source_format_version`, and `source_identity_scheme=dense_bus_id`.
- `buses.type` dictionary tokens `PQ`/`PV`/`Slack`; `controlled_bus_id` **null** = local regulation (do not write `0`).
- Native UTC timestamps; optional load/shunt `mrid` (null); dynamics `classical_params` column present (null unless mapped).
- Root metadata stamps `rpf.identity.model=hybrid_solver_flat_v1`.
- **`baseline_source_case_id`** replaces any prior `original_sentinel_case_id` wire name (null on standard PSLF planning exports).
- **Reader compatibility**: only v0.13.0 / `0.13.0`. **Re-export all goldens and case libraries.** No upgrade CLI for old `.rpf` files.
- **Dependency**: `raptrix-cim-arrow` **0.6.0** / git tag **`v0.6.0`**.

### Consumer checklist

1. Reject any file with `raptrix.version` ≠ v0.13.0.
2. Stop reading `psse_version` / `original_sentinel_case_id` / Int8 bus types / `controlled_bus_id == 0`.
3. Parse native UTC timestamps (or convert via Arrow).
4. Prefer `classical_params` when present for first-swing machines (PSLF exports leave this null today).

### Golden corpus prefers dynamic

When a `.dyd` companion exists under `tests/networks/`, `tests/golden/<stem>.rpf` is the **dynamic** conversion (parity with `raptrix-psse-rs`). The sweep also emits `<stem>_dynamic.rpf` and a no-DYD `<stem>_static.rpf`, and fails if an attached DYD yields zero `dynamics_models` rows. Regenerate with `cargo test --test golden_test -- --nocapture` or `./scripts/generate_all_rpfs.sh`.

---

## v0.5.7 / RPF v0.12.5 (additive — no migration required)

`raptrix-pslf-rs` **0.5.7** emits RPF **v0.12.5** (root metadata `raptrix.version` matches `raptrix-cim-arrow::SCHEMA_VERSION`).

### What changed

- **Nullable trailing `buses.latitude` / `buses.longitude`** (`Float64`, WGS84 degrees). PSLF EPC has no standard GIS fields — columns are always **null** on this converter path.
- **Reader compatibility**: pinned `raptrix-cim-arrow` 0.5.7 accepts **v0.12.1 through v0.12.5**.

### Compatibility

- **No re-export required** for existing **v0.12.1+** `.rpf` files.

---

## v0.5.6 / RPF v0.12.4 (additive — no migration required)

`raptrix-pslf-rs` **0.5.6** emits RPF **v0.12.4** (root metadata `raptrix.version` matches `raptrix-cim-arrow::SCHEMA_VERSION`).

### What changed

- **Version stamp only on the planning path.** v0.12.3 added nullable baseline-provenance metadata columns; v0.12.4 added optional solved-state tables. The PSLF converter leaves these null / zero-row.
- **Reader compatibility**: pinned `raptrix-cim-arrow` 0.5.6 accepts **v0.12.1 through v0.12.4**.

### Compatibility

- **No re-export required** for existing **v0.12.1+** `.rpf` files.

---

## v0.12.2 (additive — no migration required)

### What changed

- **Nullable `mrid` column** added to `branches`, `generators`, `transformers_2w`, and `transformers_3w`.
- **Schema metadata key `rpf.mrid_support = v1`** indicates stable equipment identifier support.
- **`SUPPORTED_RPF_VERSIONS`** accepts **`v0.12.2`** / **`0.12.2`** and retains **`v0.12.1`** / **`0.12.1`**.

### Compatibility

- **No re-export required.** v0.12.1 files remain readable; `mrid` columns are absent (null) in legacy files.
- **New PSLF exports** populate `mrid` with deterministic vendor-path tokens synthesized from bus/circuit identifiers when no native CIM mRID is available.
- **Downstream guidance**: prefer `mrid` over dense integer IDs for cross-system equipment mapping.

### Reader upgrade

- Accept **`v0.12.2`** in the RPF version gate.
- Read optional nullable `mrid` on equipment tables; null means legacy file without stable identifiers.

---

## v0.12.1 (breaking — re-export required)

See [CHANGELOG.md](CHANGELOG.md) for the v0.5.3 / v0.12.1 narrow-interchange release.
