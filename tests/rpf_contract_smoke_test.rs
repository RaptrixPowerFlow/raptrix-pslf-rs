// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Locked RPF interchange contract smoke tests (v0.12.1).

use std::path::Path;

use anyhow::Result;
use arrow::array::{BooleanArray, Float64Array, Int8Array};
use raptrix_cim_arrow::{
    METADATA_KEY_CASE_MODE, METADATA_KEY_DEFAULT_SHUNT_CONTROL_MODE,
    METADATA_KEY_LOADS_ZIP_FIDELITY_PRESENCE, METADATA_KEY_RPF_VERSION,
    METADATA_KEY_SOLVED_STATE_PRESENCE, METADATA_KEY_TRANSFORMER_REPRESENTATION_MODE, RPF_VERSION,
    TABLE_BUSES, TABLE_GENERATORS, TABLE_METADATA, rpf_file_metadata,
};
use raptrix_pslf_rs::{
    ExportOptions, RPF_VERSION as LIB_RPF_VERSION, write_pslf_to_rpf_with_options,
};

const EPC_PATH: &str = "tests/networks/Texas7k_20210804.EPC";
const DYD_PATH: &str = "tests/networks/Texas7k_20210804.dyd";

fn file_exists(p: &str) -> bool {
    Path::new(p).exists()
}

#[test]
fn crate_exports_rpf_version_constant() {
    assert_eq!(LIB_RPF_VERSION, RPF_VERSION);
}

#[test]
fn exported_rpf_carries_v0121_contract_metadata() -> Result<()> {
    if !file_exists(EPC_PATH) {
        eprintln!("[skip] proprietary EPC not present");
        return Ok(());
    }

    let tmp = tempfile::NamedTempFile::new()?.path().with_extension("rpf");
    let out = tmp.to_string_lossy();

    write_pslf_to_rpf_with_options(
        EPC_PATH,
        Some(DYD_PATH),
        &out,
        &ExportOptions {
            case_mode_override: Some("warm_start_planning".to_string()),
            default_shunt_control_mode_override: Some("planning_full".to_string()),
            ..ExportOptions::default()
        },
    )?;

    let root_meta = rpf_file_metadata(&tmp)?;
    assert_eq!(
        root_meta.get(METADATA_KEY_RPF_VERSION).map(String::as_str),
        Some(RPF_VERSION),
        "root rpf_version must match locked contract"
    );
    assert_eq!(
        root_meta.get(METADATA_KEY_CASE_MODE).map(String::as_str),
        Some("warm_start_planning")
    );
    assert_eq!(
        root_meta
            .get(METADATA_KEY_DEFAULT_SHUNT_CONTROL_MODE)
            .map(String::as_str),
        Some("planning_full")
    );
    assert_eq!(
        root_meta
            .get(METADATA_KEY_SOLVED_STATE_PRESENCE)
            .map(String::as_str),
        Some("not_computed")
    );
    assert_eq!(
        root_meta
            .get(METADATA_KEY_TRANSFORMER_REPRESENTATION_MODE)
            .map(String::as_str),
        Some("native_3w")
    );
    assert_eq!(
        root_meta
            .get(METADATA_KEY_LOADS_ZIP_FIDELITY_PRESENCE)
            .map(String::as_str),
        Some("not_available")
    );

    let tables: std::collections::BTreeMap<_, _> = raptrix_cim_arrow::read_rpf_tables(&tmp)?
        .into_iter()
        .collect();

    let metadata = tables.get(TABLE_METADATA).expect("metadata table");
    let is_planning = metadata
        .column_by_name("is_planning_case")
        .expect("is_planning_case")
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("Boolean");
    assert!(is_planning.value(0), "PSLF exports are planning cases");

    let buses = tables.get(TABLE_BUSES).expect("buses table");
    let bus_type = buses
        .column_by_name("type")
        .expect("type")
        .as_any()
        .downcast_ref::<Int8Array>()
        .expect("Int8");
    assert!(
        bus_type.iter().any(|v| v == Some(2)),
        "PV buses should be present"
    );

    let gens = tables.get(TABLE_GENERATORS).expect("generators table");
    let q_min = gens
        .column_by_name("q_min_mvar")
        .expect("q_min_mvar")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("Float64");
    let q_max = gens
        .column_by_name("q_max_mvar")
        .expect("q_max_mvar")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("Float64");
    for i in 0..q_min.len() {
        assert!(
            q_min.value(i) <= q_max.value(i) + 1.0e-12,
            "generator row {i}: q_min must be <= q_max"
        );
    }

    Ok(())
}
