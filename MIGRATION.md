# Migration — raptrix-pslf-rs

## v0.12.2 (additive — no migration required)

### What changed

- **Nullable `mrid` column** added to `branches`, `generators`, `transformers_2w`, and `transformers_3w`.
- **Schema metadata key `rpf.mrid_support = v1`** indicates stable equipment identifier support.
- **`SUPPORTED_RPF_VERSIONS`** accepts **`v0.12.2`** / **`0.12.2`** and retains **`v0.12.1`** / **`0.12.1`**.

### Compatibility

- **No re-export required.** v0.12.1 files remain readable; `mrid` columns are absent (null) in legacy files.
- **New PSLF exports** populate `mrid` with deterministic vendor-path tokens (`BR_`, `GEN_`, `XF2_`, `XF3_`, star-leg suffixes) synthesized from bus/circuit identifiers when no native CIM mRID is available.
- **Downstream guidance**: New `mrid` columns provide stable CIM-compatible identifiers. Downstream tools (Sentinel v2.4, Studio, etc.) should prefer `mrid` for equipment_id mapping.

### Reader upgrade

- Accept **`v0.12.2`** / **`0.12.2`** in the RPF version gate.
- Read optional nullable `mrid` on equipment tables; null means legacy file without stable identifiers.
- Prefer `mrid` over dense integer IDs for cross-system equipment mapping.

---

## v0.12.1 (breaking — re-export required)

See [CHANGELOG.md](CHANGELOG.md) for the v0.5.3 release notes. v0.12.0 and prior contract files must be re-emitted through a v0.12.1+ converter.
