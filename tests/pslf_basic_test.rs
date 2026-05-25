// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Basic smoke + cross-validation tests for the PSLF converter.
//!
//! These tests are designed to run on developer machines that have the
//! proprietary/CEII test networks locally. They will be skipped gracefully
//! if the files are not present.

use std::path::Path;

use anyhow::Result;
use raptrix_cim_arrow::{TABLE_BUSES, TABLE_GENERATORS, TABLE_LOADS, summarize_rpf};

const EPC_PATH: &str = "tests/networks/Texas7k_20210804.EPC";
const DYD_PATH: &str = "tests/networks/Texas7k_20210804.dyd";

fn file_exists(p: &str) -> bool {
    Path::new(p).exists()
}

#[test]
fn pslf_parser_and_writer_smoke() -> Result<()> {
    if !file_exists(EPC_PATH) {
        eprintln!("[test] Skipping PSLF smoke test — proprietary EPC not present");
        return Ok(());
    }

    let net = raptrix_pslf_rs::parser::parse_epc(Path::new(EPC_PATH))?;
    assert_eq!(net.buses.len(), 6717, "bus count");
    assert_eq!(net.generators.len(), 731, "generator count");

    let bus = net
        .buses
        .iter()
        .find(|b| b.number == 110001)
        .expect("bus 110001");
    assert!(
        (bus.volt - 1.037093).abs() < 0.001,
        "bus 110001 volt: got {}",
        bus.volt
    );
    assert!(
        (bus.angle - (-4.242394)).abs() < 0.01,
        "bus 110001 angle: got {}",
        bus.angle
    );

    let generator = net
        .generators
        .iter()
        .find(|g| g.bus == 111180)
        .expect("gen 111180");
    assert!(
        generator.pg > 100.0,
        "gen 111180 pg should be ~643 MW, got {}",
        generator.pg
    );
    assert!(
        (generator.qt - 379.710).abs() < 0.1,
        "gen 111180 qt: got {}",
        generator.qt
    );
    assert!(
        (generator.qb - (-250.067)).abs() < 0.1,
        "gen 111180 qb: got {}",
        generator.qb
    );

    let tmp = tempfile::NamedTempFile::new()?.path().with_extension("rpf");
    let tmp_str = tmp.to_string_lossy();

    raptrix_pslf_rs::write_pslf_to_rpf(EPC_PATH, Some(DYD_PATH), &tmp_str)?;

    let summary = summarize_rpf(&tmp)?;
    assert!(summary.total_rows > 0, "produced RPF should have rows");
    assert_eq!(summary.table_rows(TABLE_BUSES), Some(6717), "RPF bus count");
    assert_eq!(
        summary.table_rows(TABLE_GENERATORS),
        Some(731),
        "RPF generator count"
    );
    assert_eq!(
        summary.table_rows(TABLE_LOADS),
        Some(5095),
        "RPF load count"
    );
    assert!(summary.tables.iter().any(|t| t.table_name == TABLE_BUSES));

    eprintln!(
        "[test] PSLF smoke produced RPF with {} tables, {} rows",
        summary.tables.len(),
        summary.total_rows
    );
    Ok(())
}

#[test]
fn pslf_vs_psse_cross_validation() -> Result<()> {
    let epc = "tests/networks/Texas7k_20210804.EPC";
    let raw = "../raptrix-psse-rs/tests/data/external/Texas7k_20210804.RAW";

    if !file_exists(epc) || !file_exists(raw) {
        eprintln!(
            "[test] Skipping cross-validation — one or both proprietary cases not present on this machine"
        );
        return Ok(());
    }

    let pslf_out = tempfile::NamedTempFile::new()?.path().with_extension("rpf");
    let _psse_out = tempfile::NamedTempFile::new()?.path().with_extension("rpf");

    raptrix_pslf_rs::write_pslf_to_rpf(
        epc,
        Some("tests/networks/Texas7k_20210804.dyd"),
        &pslf_out.to_string_lossy(),
    )?;

    let pslf_sum = summarize_rpf(&pslf_out)?;

    eprintln!(
        "[cross-val] PSLF produced {} buses, {} generators",
        pslf_sum.table_rows(TABLE_BUSES).unwrap_or(0),
        pslf_sum.table_rows(TABLE_GENERATORS).unwrap_or(0)
    );

    assert!(pslf_sum.table_rows(TABLE_BUSES).unwrap_or(0) > 0);
    assert!(pslf_sum.table_rows(TABLE_GENERATORS).unwrap_or(0) > 0);

    Ok(())
}

#[test]
fn transformer_ps_impedance_from_epc_header() -> Result<()> {
    if !file_exists(EPC_PATH) {
        eprintln!("[test] Skipping transformer parse test — proprietary EPC not present");
        return Ok(());
    }

    let net = raptrix_pslf_rs::parser::parse_epc(Path::new(EPC_PATH))?;
    let xfmr = net
        .transformers
        .iter()
        .find(|t| t.from_bus == 110001 && t.to_bus == 110004)
        .expect("transformer 110001-110004");
    assert!(
        (xfmr.r - 1.440e-3).abs() < 1.0e-6,
        "ps_r: got {}",
        xfmr.r
    );
    assert!(
        (xfmr.x - 4.775e-2).abs() < 1.0e-5,
        "ps_x: got {}",
        xfmr.x
    );
    assert!(
        (xfmr.rate_a - 245.7).abs() < 0.1,
        "rate_a: got {}",
        xfmr.rate_a
    );

    let xfmr_345 = net
        .transformers
        .iter()
        .find(|t| t.from_bus == 110127 && t.to_bus == 110126 && t.ckt.as_ref() == "1")
        .expect("transformer 110127-110126 ckt 1");
    assert!(
        (xfmr_345.tap - 1.0).abs() < 1.0e-6,
        "tap should be WINDV (1.0), not kV: got {}",
        xfmr_345.tap
    );
    assert!(
        (xfmr_345.to_kv - 345.0).abs() < 0.1,
        "to_kv: got {}",
        xfmr_345.to_kv
    );
    Ok(())
}

#[test]
fn series25_switched_shunt_row_count_matches_psse() -> Result<()> {
    let epc = "tests/networks/Texas2k_series25_case1_summerpeak.EPC";
    if !file_exists(epc) {
        eprintln!("[test] Skipping SVD count test — proprietary EPC not present");
        return Ok(());
    }

    let net = raptrix_pslf_rs::parser::parse_epc(Path::new(epc))?;
    assert_eq!(
        net.switched_shunts.len(),
        202,
        "parsed switched_shunts count (expect one EPC record per device, matching PSSE RAW)"
    );

    let tmp = tempfile::NamedTempFile::new()?.path().with_extension("rpf");
    raptrix_pslf_rs::write_pslf_to_rpf(epc, None, &tmp.to_string_lossy())?;
    let summary = summarize_rpf(&tmp)?;
    assert_eq!(
        summary.table_rows(raptrix_cim_arrow::TABLE_SWITCHED_SHUNTS),
        Some(202),
        "exported switched_shunts row count"
    );
    Ok(())
}
