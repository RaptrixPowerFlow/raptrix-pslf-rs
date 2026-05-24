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
