// Row-count parity tests against psse-rs reference RPFs (requires local test data).

use std::fs;
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
    let compare = repo_root()
        .join("tests/compare/psse")
        .join(format!("{name}.rpf"));
    if compare.exists() {
        return compare;
    }
    repo_root()
        .join("../raptrix-psse-rs/tests/data/external")
        .join(format!("{name}.rpf"))
}

fn first_local_pair() -> Option<(PathBuf, Option<PathBuf>, PathBuf)> {
    let networks = repo_root().join("tests/networks");
    let entries = fs::read_dir(&networks).ok()?;
    let mut epcs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("epc"))
        })
        .collect();
    epcs.sort();
    for epc in epcs {
        let stem = epc.file_stem()?.to_str()?;
        let rpf = psse_reference_rpf(stem);
        if !rpf.exists() {
            continue;
        }
        let version = rpf_file_metadata(&rpf)
            .ok()
            .and_then(|meta| meta.get("rpf_version").cloned())
            .unwrap_or_default();
        if version != RPF_VERSION {
            continue;
        }
        let dyd = companion(&networks, stem, &["dyd"]);
        return Some((epc, dyd, rpf));
    }
    None
}

fn companion(dir: &Path, stem: &str, exts: &[&str]) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let ext_ok = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| exts.iter().any(|want| ext.eq_ignore_ascii_case(want)));
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            ext_ok
                && (name.eq_ignore_ascii_case(stem)
                    || name
                        .to_ascii_lowercase()
                        .starts_with(&format!("{}_", stem.to_ascii_lowercase())))
        })
        .collect();
    matches.sort_by_key(|path| path.file_name().map(|n| n.len()).unwrap_or(usize::MAX));
    matches.into_iter().next()
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
fn local_core_table_parity_when_reference_exists() -> Result<()> {
    let Some((epc, dyd, psse_rpf)) = first_local_pair() else {
        eprintln!("[skip] no local EPC with a same-version reference RPF");
        return Ok(());
    };

    let tmp = TempDir::new()?;
    let out = tmp.path().join("converted.rpf");
    write_pslf_to_rpf_with_options(
        epc.to_str().unwrap(),
        dyd.as_ref().and_then(|path| path.to_str()),
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
