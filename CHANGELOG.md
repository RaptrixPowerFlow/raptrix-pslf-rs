# Changelog — raptrix-pslf-rs

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- Project scaffold: exact structural and stylistic mirror of `raptrix-psse-rs` v0.4.0.
- CLI skeleton with `--epc` / `--dyd` (user-chosen closest parallel to psse-rs `--raw` / `--dyr`), `convert`, `view`, and `validate` subcommands.
- Strong `.gitignore` protecting CEII/proprietary `tests/networks/` and all vendor powerflow formats.
- MPL-2.0 license and initial documentation aligned with the Raptrix Suite marketing guide.

### Notes
- First public version will be tagged after parser + full table export parity is achieved.
- `raptrix-cim-arrow` pinned to the same rev as current psse-rs for interchange compatibility.

---

## [0.1.0] — Initial Release (target)

- Full EPC + DYD parser.
- Complete mapping to all 18 canonical RPF v0.10.0 tables + dynamics.
- Cross-tool equivalence tests with psse-rs on overlapping Texas cases.
- Packaging scripts and pre-release hygiene parity.
