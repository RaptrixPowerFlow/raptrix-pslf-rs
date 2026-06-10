// Row-count parity tests against psse-rs reference RPFs (requires local test data).

use std::path::{Path, PathBuf};

use anyhow::Result;
use raptrix_cim_arrow::{
    RPF_VERSION, TABLE_AREAS, TABLE_BRANCHES, TABLE_BUSES, TABLE_GENERATORS, TABLE_LOADS,
    TABLE_OWNERS, TABLE_SWITCHED_SHUNTS, TABLE_TRANSFORMERS_2W, TABLE_ZONES, rpf_file_metadata,
    summarize_rpf,
};
use raptrix_pslf_rs::{ExportOptions, write_pslf_to_rpf_with_options};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn psse_reference_rpf(name: &str) -> PathBuf {
    let compare = repo_root().join(format!("tests/compare/psse/{name}.rpf"));
    if compare.exists() {
        return compare;
    }
    repo_root()
        .join("../raptrix-psse-rs/tests/data/external")
        .join(format!("{name}.rpf"))
}

fn compare_table_rows(pslf: &Path, psse: &Path, table: &str) -> Result<(usize, usize)> {
    let left = summarize_rpf(pslf)?;
    let right = summarize_rpf(psse)?;
    let l = left
        .tables
        .iter()
        .find(|t| t.table_name == table)
        .map(|t| t.rows)
        .unwrap_or(0);
    let r = right
        .tables
        .iter()
        .find(|t| t.table_name == table)
        .map(|t| t.rows)
        .unwrap_or(0);
    Ok((l, r))
}

#[test]
fn texas7k_core_table_parity() -> Result<()> {
    let epc = repo_root().join("tests/networks/Texas7k_20210804.EPC");
    let dyd = repo_root().join("tests/networks/Texas7k_20210804.dyd");
    let psse_rpf = psse_reference_rpf("Texas7k_20210804");

    if !epc.exists() {
        eprintln!("[skip] missing EPC: {}", epc.display());
        return Ok(());
    }
    if !psse_rpf.exists() {
        eprintln!(
            "[skip] missing PSSE reference RPF (run scripts/compare-psse-rpf.sh first): {}",
            psse_rpf.display()
        );
        return Ok(());
    }

    let psse_meta = rpf_file_metadata(&psse_rpf)?;
    let psse_version = psse_meta
        .get("rpf_version")
        .map(String::as_str)
        .unwrap_or("");
    if psse_version != RPF_VERSION {
        eprintln!(
            "[skip] PSSE reference RPF is {psse_version} (need {RPF_VERSION}); \
             regenerate with scripts/compare-psse-rpf.ps1: {}",
            psse_rpf.display()
        );
        return Ok(());
    }

    let tmp = TempDir::new()?;
    let out = tmp.path().join("Texas7k.rpf");
    write_pslf_to_rpf_with_options(
        epc.to_str().unwrap(),
        dyd.exists().then(|| dyd.to_str().unwrap()),
        out.to_str().unwrap(),
        &ExportOptions::default(),
    )?;

    for table in [
        TABLE_BUSES,
        TABLE_BRANCHES,
        TABLE_GENERATORS,
        TABLE_LOADS,
        TABLE_TRANSFORMERS_2W,
        TABLE_AREAS,
        TABLE_ZONES,
        TABLE_OWNERS,
    ] {
        let (l, r) = compare_table_rows(&out, &psse_rpf, table)?;
        assert_eq!(l, r, "table {table}: PSLF={l} PSSE={r}");
    }

    // switched_shunts: PSLF EPC has 634 SVD devices vs PSSE RAW 429 — known semantic gap
    // (more granular SVD records in EPC; both are valid representations of the same network)
    let (svd_l, svd_r) = compare_table_rows(&out, &psse_rpf, TABLE_SWITCHED_SHUNTS)?;
    assert!(
        svd_l > 0,
        "PSLF should have switched_shunts rows (got {svd_l})"
    );
    assert!(
        svd_r > 0,
        "PSSE should have switched_shunts rows (got {svd_r})"
    );

    Ok(())
}
