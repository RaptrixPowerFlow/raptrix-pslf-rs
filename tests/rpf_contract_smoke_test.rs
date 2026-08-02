// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Locked RPF interchange contract smoke tests (v0.13.0).

use std::path::Path;

use anyhow::Result;
use arrow::array::{
    Array, BooleanArray, DictionaryArray, Float64Array, StringArray, TimestampMicrosecondArray,
};
use arrow::datatypes::Int32Type;
use raptrix_cim_arrow::{
    BUS_TYPE_PV, BUS_TYPE_SLACK, IDENTITY_MODEL_HYBRID_SOLVER_FLAT_V1, METADATA_KEY_CASE_MODE,
    METADATA_KEY_DEFAULT_SHUNT_CONTROL_MODE, METADATA_KEY_IDENTITY_MODEL,
    METADATA_KEY_LOADS_ZIP_FIDELITY_PRESENCE, METADATA_KEY_MRID_SUPPORT, METADATA_KEY_RPF_VERSION,
    METADATA_KEY_SOLVED_STATE_PRESENCE, METADATA_KEY_TRANSFORMER_REPRESENTATION_MODE, RPF_VERSION,
    TABLE_BRANCHES, TABLE_BUSES, TABLE_GENERATORS, TABLE_METADATA, rpf_file_metadata, table_schema,
};
use raptrix_pslf_rs::{
    ExportOptions, RPF_VERSION as LIB_RPF_VERSION, write_pslf_to_rpf_with_options,
};

const EPC_PATH: &str = "tests/networks/Texas7k_20210804.EPC";
const DYD_PATH: &str = "tests/networks/Texas7k_20210804.dyd";

fn file_exists(p: &str) -> bool {
    Path::new(p).exists()
}

fn dict_utf8_at(col: &dyn Array, i: usize) -> &str {
    let dict = col
        .as_any()
        .downcast_ref::<DictionaryArray<Int32Type>>()
        .expect("expected Dictionary<Int32, Utf8>");
    assert!(!dict.is_null(i), "dictionary entry {i} must be non-null");
    let values = dict
        .values()
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("dictionary values must be Utf8");
    values.value(dict.key(i).expect("dictionary key"))
}

#[test]
fn crate_exports_rpf_version_constant() {
    assert_eq!(LIB_RPF_VERSION, RPF_VERSION);
    assert_eq!(RPF_VERSION, "v0.13.0");
}

#[test]
fn generators_schema_includes_trailing_mrid_column() {
    let schema = table_schema(TABLE_GENERATORS).expect("generators schema");
    assert_eq!(schema.fields().len(), 26);
    assert_eq!(schema.field(25).name(), "mrid");
    assert!(schema.field(24).is_nullable(), "controlled_bus_id nullable");
}

#[test]
fn exported_rpf_carries_v0130_contract_metadata() -> Result<()> {
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
        root_meta
            .get(METADATA_KEY_IDENTITY_MODEL)
            .map(String::as_str),
        Some(IDENTITY_MODEL_HYBRID_SOLVER_FLAT_V1),
    );
    assert_eq!(
        root_meta.get(METADATA_KEY_MRID_SUPPORT).map(String::as_str),
        Some("v1")
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

    assert!(
        metadata.schema().field_with_name("psse_version").is_err(),
        "v0.13.0 must not expose psse_version"
    );
    assert!(
        metadata
            .schema()
            .field_with_name("baseline_source_case_id")
            .is_ok(),
        "baseline_source_case_id must be present"
    );
    assert!(
        metadata
            .schema()
            .field_with_name("original_sentinel_case_id")
            .is_err(),
        "original_sentinel_case_id must not appear on the wire"
    );

    let source_format = metadata
        .column_by_name("source_format")
        .expect("source_format");
    assert_eq!(dict_utf8_at(source_format.as_ref(), 0), "pslf_epc");
    let source_identity = metadata
        .column_by_name("source_identity_scheme")
        .expect("source_identity_scheme");
    assert_eq!(dict_utf8_at(source_identity.as_ref(), 0), "dense_bus_id");

    let timestamp_utc = metadata
        .column_by_name("timestamp_utc")
        .expect("timestamp_utc")
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()
        .expect("Timestamp(us, UTC)");
    assert!(timestamp_utc.is_valid(0));

    let buses = tables.get(TABLE_BUSES).expect("buses table");
    let bus_type = buses.column_by_name("type").expect("type");
    assert!(
        (0..bus_type.len()).any(|i| dict_utf8_at(bus_type.as_ref(), i) == BUS_TYPE_PV),
        "PV buses should be present"
    );
    assert!(
        (0..bus_type.len()).any(|i| dict_utf8_at(bus_type.as_ref(), i) == BUS_TYPE_SLACK),
        "explicit Slack (PSLF ty=0) must be exported — no auto-slack"
    );

    let gens = tables.get(TABLE_GENERATORS).expect("generators table");
    assert_eq!(gens.schema().fields().len(), 26);
    assert_eq!(gens.schema().field(25).name(), "mrid");
    assert!(gens.schema().field(24).is_nullable());

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

    let gen_mrid = gens
        .column_by_name("mrid")
        .expect("mrid")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("Utf8");
    assert!(
        (0..gen_mrid.len()).any(|i| gen_mrid.is_valid(i)),
        "at least one generator row must carry non-null mrid"
    );

    let branches = tables.get(TABLE_BRANCHES).expect("branches table");
    assert_eq!(branches.schema().fields().len(), 28);
    assert_eq!(branches.schema().field(27).name(), "mrid");
    let branch_mrid = branches
        .column_by_name("mrid")
        .expect("mrid")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("Utf8");
    assert!(
        (0..branch_mrid.len()).any(|i| branch_mrid.is_valid(i)),
        "at least one branch row must carry non-null mrid"
    );

    Ok(())
}
