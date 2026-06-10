// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Golden integration sweep for proprietary EPC reference cases (local machines only).
//! Outputs land in `tests/golden/<case-stem>.rpf` (gitignored).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use raptrix_cim_arrow::{RPF_VERSION, TABLE_BRANCHES, TABLE_BUSES, TABLE_GENERATORS, TABLE_LOADS};

const NETWORKS_DIR: &str = "tests/networks";
const GOLDEN_DIR: &str = "tests/golden";

#[derive(Debug)]
struct CaseTiming {
    case_name: String,
    epc_file: String,
    dynamics_file: Option<String>,
    output_file: String,
    elapsed_ms: u128,
    buses: usize,
    branches: usize,
    generators: usize,
    loads: usize,
    total_rows: usize,
}

fn rows(summary: &raptrix_cim_arrow::RpfSummary, table_name: &str) -> usize {
    summary
        .tables
        .iter()
        .find(|t| t.table_name == table_name)
        .map(|t| t.rows)
        .unwrap_or(0)
}

fn is_extension(path: &Path, want: &[&str]) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    want.iter().any(|w| ext.eq_ignore_ascii_case(w))
}

fn stem_string(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(std::string::ToString::to_string)
}

fn discover_files_by_ext(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_extension(&path, exts) {
            out.push(path);
        }
    }

    out.sort_by(|a, b| {
        a.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .cmp(
                &b.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase(),
            )
    });

    out
}

fn find_dynamic_companion(epc: &Path, dynamic_files: &[PathBuf]) -> Option<PathBuf> {
    let epc_stem = stem_string(epc)?.to_ascii_lowercase();

    let mut exact: Option<&PathBuf> = None;
    let mut prefix_matches: Vec<&PathBuf> = Vec::new();

    for dyn_path in dynamic_files {
        let Some(dyn_stem) = stem_string(dyn_path).map(|s| s.to_ascii_lowercase()) else {
            continue;
        };

        if dyn_stem == epc_stem {
            exact = Some(dyn_path);
            break;
        }

        if dyn_stem.starts_with(&(epc_stem.clone() + "_")) {
            prefix_matches.push(dyn_path);
        }
    }

    if let Some(found) = exact {
        return Some(found.clone());
    }

    prefix_matches
        .into_iter()
        .min_by_key(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or("").len())
        .cloned()
}

fn run_case(
    epc: &Path,
    dynamic_files: &[PathBuf],
    golden_dir: &Path,
) -> Result<CaseTiming, String> {
    let case_name =
        stem_string(epc).ok_or_else(|| format!("invalid EPC filename: {}", epc.display()))?;
    let dyn_path = find_dynamic_companion(epc, dynamic_files);
    let out_path = golden_dir.join(format!("{case_name}.rpf"));

    let epc_s = epc.to_string_lossy().to_string();
    let out_s = out_path.to_string_lossy().to_string();
    let dyn_s = dyn_path.as_ref().map(|p| p.to_string_lossy().to_string());

    let t0 = Instant::now();
    raptrix_pslf_rs::write_pslf_to_rpf(&epc_s, dyn_s.as_deref(), &out_s)
        .map_err(|e| format!("conversion failed: {e:#}"))?;
    let elapsed_ms = t0.elapsed().as_millis();

    let summary = raptrix_cim_arrow::summarize_rpf(Path::new(&out_s))
        .map_err(|e| format!("summarize_rpf failed: {e:#}"))?;
    if !summary.has_all_canonical_tables {
        return Err("missing canonical root tables".to_string());
    }

    let metadata = raptrix_cim_arrow::rpf_file_metadata(Path::new(&out_s))
        .map_err(|e| format!("rpf_file_metadata failed: {e:#}"))?;
    let rpf_version = metadata
        .get("rpf_version")
        .map(|v| v.as_str())
        .unwrap_or("");
    if rpf_version != RPF_VERSION {
        return Err(format!(
            "rpf_version mismatch: expected {RPF_VERSION}, got {rpf_version}"
        ));
    }

    let buses = rows(&summary, TABLE_BUSES);
    let branches = rows(&summary, TABLE_BRANCHES);
    let generators = rows(&summary, TABLE_GENERATORS);
    let loads = rows(&summary, TABLE_LOADS);

    if buses == 0 || branches == 0 || generators == 0 || loads == 0 {
        return Err(format!(
            "unexpected empty core table(s): buses={buses} branches={branches} generators={generators} loads={loads}"
        ));
    }

    Ok(CaseTiming {
        case_name,
        epc_file: epc_s,
        dynamics_file: dyn_s,
        output_file: out_s,
        elapsed_ms,
        buses,
        branches,
        generators,
        loads,
        total_rows: summary.total_rows,
    })
}

#[test]
fn golden_build_all_network_epc_cases() {
    assert_eq!(RPF_VERSION, "v0.12.1");

    let networks_dir = Path::new(NETWORKS_DIR);
    if !networks_dir.exists() {
        eprintln!("[skip] {} not found", networks_dir.display());
        return;
    }

    fs::create_dir_all(GOLDEN_DIR).expect("failed to create tests/golden output directory");

    let epc_files = discover_files_by_ext(networks_dir, &["epc"]);
    let dynamic_files = discover_files_by_ext(networks_dir, &["dyd"]);

    if epc_files.is_empty() {
        eprintln!("[skip] no EPC files found under {}", networks_dir.display());
        return;
    }

    let mut timings: Vec<CaseTiming> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    let t_total = Instant::now();

    for epc in &epc_files {
        let epc_name = epc
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>");
        match run_case(epc, &dynamic_files, Path::new(GOLDEN_DIR)) {
            Ok(t) => {
                eprintln!(
                    "[ok] {:45} {:8} ms  dyn={}  out={}",
                    epc_name,
                    t.elapsed_ms,
                    t.dynamics_file
                        .as_deref()
                        .map(|p| Path::new(p)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("?"))
                        .unwrap_or("none"),
                    Path::new(&t.output_file)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("?")
                );
                timings.push(t);
            }
            Err(err) => {
                failures.push(format!("{} -> {}", epc.display(), err));
            }
        }
    }

    let total_elapsed_ms = t_total.elapsed().as_millis();

    timings.sort_by_key(|t| std::cmp::Reverse(t.elapsed_ms));

    eprintln!("\n=== Golden Build Timings (slowest first) ===");
    for t in &timings {
        eprintln!(
            "{:40} {:8} ms  buses={:<7} branches={:<7} gens={:<7} loads={:<7} rows={}",
            t.case_name, t.elapsed_ms, t.buses, t.branches, t.generators, t.loads, t.total_rows
        );
    }

    eprintln!("\n=== Golden Build Totals ===");
    eprintln!("  EPC files discovered : {}", epc_files.len());
    eprintln!("  Successful builds    : {}", timings.len());
    eprintln!("  Failed builds        : {}", failures.len());
    eprintln!("  Total elapsed        : {} ms", total_elapsed_ms);

    if !failures.is_empty() {
        eprintln!("\n=== Failures ===");
        for failure in &failures {
            eprintln!("  - {failure}");
        }
        panic!("{} network EPC case(s) failed", failures.len());
    }

    for t in &timings {
        assert!(
            Path::new(&t.output_file).exists(),
            "missing expected output {}",
            t.output_file
        );
        let expected_name = format!("{}.rpf", t.case_name);
        let actual_name = Path::new(&t.output_file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        assert_eq!(
            actual_name, expected_name,
            "unexpected output filename policy"
        );
        assert!(Path::new(&t.epc_file).exists(), "source EPC must exist");
    }
}
