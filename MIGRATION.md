# Migration — raptrix-pslf-rs

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
