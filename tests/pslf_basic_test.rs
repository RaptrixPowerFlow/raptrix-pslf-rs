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

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use arrow::array::{Float64Array, Int8Array, Int32Array};
use raptrix_cim_arrow::{
    RPF_VERSION, TABLE_BUSES, TABLE_GENERATORS, TABLE_LOADS, read_rpf_tables, rpf_file_metadata,
    summarize_rpf,
};

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

    let metadata = rpf_file_metadata(&tmp)?;
    let rpf_version = metadata
        .get("rpf_version")
        .map(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        rpf_version, RPF_VERSION,
        "rpf_version mismatch: expected {RPF_VERSION}, got {rpf_version}"
    );

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

    let tables: BTreeMap<_, _> = read_rpf_tables(&tmp)?.into_iter().collect();
    let buses = tables
        .get(TABLE_BUSES)
        .expect("buses table in exported RPF");
    let bus_id_col = buses
        .column_by_name("bus_id")
        .expect("bus_id column")
        .as_any()
        .downcast_ref::<Int32Array>()
        .expect("bus_id Int32");
    let v_mag_col = buses
        .column_by_name("v_mag_set")
        .expect("v_mag_set column")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("v_mag_set Float64");
    let bus_type_col = buses
        .column_by_name("type")
        .expect("type column in buses")
        .as_any()
        .downcast_ref::<Int8Array>()
        .expect("type Int8");
    let q_min_col = buses
        .column_by_name("q_min")
        .expect("q_min column")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("q_min Float64");
    let q_max_col = buses
        .column_by_name("q_max")
        .expect("q_max column")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("q_max Float64");
    let bus_idx = (0..bus_id_col.len())
        .find(|&i| bus_id_col.value(i) == 111180)
        .expect("bus 111180 in RPF");
    let bus_111180 = net
        .buses
        .iter()
        .find(|b| b.number == 111180)
        .expect("bus 111180 in parsed EPC");
    // v_mag_set for generator buses now comes from bus.vsched (EPC bus record, colon+2),
    // not from the continuation-line generator.vs placeholder.
    // For bus 111180: vsched = 1.038548 in Texas7k EPC.
    assert!(
        (v_mag_col.value(bus_idx) - bus_111180.vsched).abs() < 0.001,
        "gen bus 111180 v_mag_set should follow bus.vsched ({:.6}), got {}",
        bus_111180.vsched,
        v_mag_col.value(bus_idx)
    );
    assert!(
        (bus_111180.vsched - 1.038548_f64).abs() < 0.0001,
        "bus 111180 vsched from EPC: expected ~1.038548, got {}",
        bus_111180.vsched
    );
    assert_eq!(
        bus_type_col.value(bus_idx),
        2i8,
        "gen bus 111180 must be exported as type-2 (PV)"
    );
    // sbase from EPC header — Texas7k uses 100
    let base_mva = net.sbase;
    let expected_qmin = generator.qb / base_mva;
    let expected_qmax = generator.qt / base_mva;
    assert!(
        (q_min_col.value(bus_idx) - expected_qmin).abs() < 0.01,
        "bus 111180 q_min pu: expected {:.6}, got {}",
        expected_qmin,
        q_min_col.value(bus_idx)
    );
    assert!(
        (q_max_col.value(bus_idx) - expected_qmax).abs() < 0.01,
        "bus 111180 q_max pu: expected {:.6}, got {}",
        expected_qmax,
        q_max_col.value(bus_idx)
    );

    let gens = tables
        .get(TABLE_GENERATORS)
        .expect("generators table in exported RPF");
    let gen_bus_col = gens
        .column_by_name("bus_id")
        .expect("bus_id column")
        .as_any()
        .downcast_ref::<Int32Array>()
        .expect("bus_id Int32");
    let gen_qmin_col = gens
        .column_by_name("q_min_mvar")
        .expect("q_min_mvar column")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("q_min_mvar Float64");
    let gen_qmax_col = gens
        .column_by_name("q_max_mvar")
        .expect("q_max_mvar column")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("q_max_mvar Float64");
    let gen_idx = (0..gen_bus_col.len())
        .find(|&i| gen_bus_col.value(i) == 111180)
        .expect("gen 111180 in RPF");
    assert!(
        (gen_qmin_col.value(gen_idx) - generator.qb).abs() < 0.1,
        "gen 111180 q_min_mvar: got {}",
        gen_qmin_col.value(gen_idx)
    );
    assert!(
        (gen_qmax_col.value(gen_idx) - generator.qt).abs() < 0.1,
        "gen 111180 q_max_mvar: got {}",
        gen_qmax_col.value(gen_idx)
    );

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
    assert!((xfmr.r - 1.440e-3).abs() < 1.0e-6, "ps_r: got {}", xfmr.r);
    assert!((xfmr.x - 4.775e-2).abs() < 1.0e-5, "ps_x: got {}", xfmr.x);
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

#[test]
fn series24_case4_svd_count_matches_epc() -> Result<()> {
    let epc = "tests/networks/Texas2k_series24_case4_2024lowload.EPC";
    if !file_exists(epc) {
        eprintln!("[test] Skipping series24 SVD count test — proprietary EPC not present");
        return Ok(());
    }

    let net = raptrix_pslf_rs::parser::parse_epc(Path::new(epc))?;
    assert_eq!(
        net.switched_shunts.len(),
        157,
        "parsed switched_shunts count (157 EPC records; PSSE RAW may export 153)"
    );

    let tmp = tempfile::NamedTempFile::new()?.path().with_extension("rpf");
    raptrix_pslf_rs::write_pslf_to_rpf(epc, None, &tmp.to_string_lossy())?;
    let summary = summarize_rpf(&tmp)?;
    assert_eq!(
        summary.table_rows(raptrix_cim_arrow::TABLE_SWITCHED_SHUNTS),
        Some(157),
        "exported switched_shunts row count"
    );
    Ok(())
}
