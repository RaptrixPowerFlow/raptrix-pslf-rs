// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Locked RPF interchange contract smoke tests (v0.14.5).

use std::collections::HashMap;

use anyhow::Result;
use arrow::array::{
    Array, BooleanArray, DictionaryArray, Float64Array, StringArray, TimestampMicrosecondArray,
};
use arrow::datatypes::Int32Type;
use raptrix_cim_arrow::{
    BUS_TYPE_PV, BUS_TYPE_SLACK, IDENTITY_MODEL_HYBRID_SOLVER_FLAT_V1, METADATA_KEY_CASE_MODE,
    METADATA_KEY_COMPUTATIONAL_LOAD_MODE, METADATA_KEY_DEFAULT_SHUNT_CONTROL_MODE,
    METADATA_KEY_FEATURE_DC_CONVERTERS, METADATA_KEY_IDENTITY_MODEL,
    METADATA_KEY_LOADS_ZIP_FIDELITY_PRESENCE, METADATA_KEY_MRID_SUPPORT, METADATA_KEY_RPF_VERSION,
    METADATA_KEY_SOLVED_STATE_PRESENCE, METADATA_KEY_TRANSFORMER_REPRESENTATION_MODE, RPF_VERSION,
    TABLE_BRANCHES, TABLE_BUSES, TABLE_COMPUTATIONAL_LOAD_PROFILES, TABLE_CONTINGENCIES,
    TABLE_CONTINGENCY_SEQUENCES, TABLE_DC_CONVERTERS, TABLE_DC_LINES_2W, TABLE_GENERATORS,
    TABLE_METADATA, TABLE_SWITCHED_SHUNTS, rpf_file_metadata, table_schema,
};
use raptrix_pslf_rs::export::build_branches_batch;
use raptrix_pslf_rs::models::Branch;
use raptrix_pslf_rs::{
    ExportOptions, RPF_VERSION as LIB_RPF_VERSION, write_pslf_to_rpf_with_options,
};

const SYNTHETIC_EPC: &str = "\
title
synthetic
solution parameters
sbase 100
bus data
1 SLACK 138 0 0 0 : 0 1.01 1.02 -5 1 1
2 LOAD 138 0 0 0 : 1 1.00 1.00 0 1 1
3 GEN 13.8 0 0 0 : 2 1.05 1.01 2 1 1
load data
2 LOAD 138 1 : 1 50 10
generator data
3 GEN 13.8 1 : 1 0 0 0 0 0 0 0 40 80 10 5 60 -10 100
branch data
1 A 138 2 B 138 1 : 1 0.01 0.20 0.05 100 110 120
svd data
2 SVD 138 0 0 1 : 1 1 0 0 0 0 0 0 0.40 0 0 0.02 /
3 0.25 4 0.50 0 1 1
end
";

fn write_synthetic_epc() -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::with_suffix(".epc").expect("temp epc");
    std::io::Write::write_all(&mut file, SYNTHETIC_EPC.as_bytes()).unwrap();
    file
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
    assert!(
        matches!(RPF_VERSION, "v0.14.5"),
        "unexpected RPF_VERSION {RPF_VERSION}"
    );
}

#[test]
fn branch_batch_appends_null_end_shunts() {
    let branch = Branch {
        from_bus: 1,
        to_bus: 2,
        ckt: "1".into(),
        r: 0.01,
        x: 0.1,
        b: 0.00609,
        rate_a: 100.0,
        rate_b: 110.0,
        rate_c: 120.0,
        status: 1,
        ..Branch::default()
    };
    let mut nominal_kv = HashMap::new();
    nominal_kv.insert(1, 138.0);
    nominal_kv.insert(2, 138.0);

    let batch = build_branches_batch(&[branch], &nominal_kv, 100.0).expect("branches batch");
    assert_eq!(batch.num_columns(), 36);
    assert_eq!(batch.schema().field(27).name(), "mrid");
    assert_eq!(batch.schema().field(28).name(), "is_secured");
    assert_eq!(batch.schema().field(31).name(), "is_bptf");
    assert_eq!(batch.schema().field(32).name(), "g_from");
    assert_eq!(batch.schema().field(33).name(), "b_from");
    assert_eq!(batch.schema().field(34).name(), "g_to");
    assert_eq!(batch.schema().field(35).name(), "b_to");

    let b_shunt = batch
        .column_by_name("b_shunt")
        .expect("b_shunt")
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("Float64");
    assert!(
        (b_shunt.value(0) - 0.00609).abs() < 1.0e-12,
        "b_shunt stays total line charging"
    );

    for name in [
        "is_secured",
        "is_bes",
        "is_bps",
        "is_bptf",
        "g_from",
        "b_from",
        "g_to",
        "b_to",
    ] {
        let col = batch
            .column_by_name(name)
            .unwrap_or_else(|| panic!("{name}"));
        assert!(col.is_null(0), "{name} must be null, not 0.0");
        assert_eq!(col.null_count(), col.len());
    }
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
    let epc = write_synthetic_epc();
    let epc_path = epc.path().to_str().expect("utf-8 temp path");

    let tmp = tempfile::NamedTempFile::new()?.path().with_extension("rpf");
    let out = tmp.to_string_lossy();

    write_pslf_to_rpf_with_options(
        epc_path,
        None,
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
        root_meta.get(METADATA_KEY_RPF_VERSION).map(String::as_str),
        Some("v0.14.5")
    );
    assert_eq!(
        root_meta
            .get(METADATA_KEY_FEATURE_DC_CONVERTERS)
            .map(String::as_str),
        Some("true")
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

    let load_mode = metadata
        .column_by_name(METADATA_KEY_COMPUTATIONAL_LOAD_MODE)
        .expect("computational_load_mode");
    assert_eq!(
        load_mode.null_count(),
        load_mode.len(),
        "computational_load_mode stays null"
    );

    let branches = tables.get(TABLE_BRANCHES).expect("branches table");
    assert_eq!(branches.schema().fields().len(), 36);
    assert_eq!(branches.schema().field(27).name(), "mrid");
    assert_eq!(branches.schema().field(28).name(), "is_secured");
    assert_eq!(branches.schema().field(31).name(), "is_bptf");
    assert_eq!(branches.schema().field(32).name(), "g_from");
    assert_eq!(branches.schema().field(33).name(), "b_from");
    assert_eq!(branches.schema().field(34).name(), "g_to");
    assert_eq!(branches.schema().field(35).name(), "b_to");
    for name in [
        "is_secured",
        "is_bes",
        "is_bps",
        "is_bptf",
        "g_from",
        "b_from",
        "g_to",
        "b_to",
    ] {
        let col = branches
            .column_by_name(name)
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(
            col.null_count(),
            col.len(),
            "{name} must be all-null from converter"
        );
    }
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

    let switched = tables
        .get(TABLE_SWITCHED_SHUNTS)
        .expect("switched_shunts table");
    for name in ["shunt_control_mode", "regulated_bus_id"] {
        let col = switched.column_by_name(name).expect(name);
        assert_eq!(
            col.null_count(),
            col.len(),
            "{name} must be all-null (do not invent PSS/E MODSW)"
        );
    }

    let contingencies = tables
        .get(TABLE_CONTINGENCIES)
        .expect("contingencies table");
    assert_eq!(contingencies.schema().fields().len(), 10);
    assert_eq!(contingencies.schema().field(8).name(), "tpl_category");
    assert_eq!(contingencies.schema().field(9).name(), "reserved");
    assert_eq!(contingencies.num_rows(), 0);
    assert!(
        !tables.contains_key(TABLE_CONTINGENCY_SEQUENCES),
        "PSLF path must omit contingency_sequences"
    );
    assert!(
        !tables.contains_key(TABLE_COMPUTATIONAL_LOAD_PROFILES),
        "PSLF path must omit computational_load_profiles"
    );

    let dc_lines = tables.get(TABLE_DC_LINES_2W).expect("dc_lines_2w");
    assert_eq!(dc_lines.num_columns(), 15);
    assert_eq!(dc_lines.num_rows(), 0);
    let converters = tables.get(TABLE_DC_CONVERTERS).expect("dc_converters");
    assert_eq!(converters.num_rows(), 0);

    Ok(())
}
