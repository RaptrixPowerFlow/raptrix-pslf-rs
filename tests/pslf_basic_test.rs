// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Behavior tests on a synthetic EPC. Licensed decks are not named or required.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use arrow::array::{Array, Float64Array, Int32Array, StringArray};
use raptrix_cim_arrow::{
    BUS_TYPE_PQ, BUS_TYPE_PV, BUS_TYPE_SLACK, RPF_VERSION, TABLE_BUSES, TABLE_GENERATORS,
    TABLE_SWITCHED_SHUNT_BANKS, TABLE_SWITCHED_SHUNTS, read_rpf_tables, rpf_file_metadata,
};
use raptrix_pslf_rs::{TransformerRepresentationMode, parser, validate_pslf_epc};

const SYNTHETIC_EPC: &str = "\
title
synthetic
solution parameters
sbase 100
bus data
1 SLACK 138 0 0 0 : 0 1.01 1.02 -5 1 1
2 LOAD 138 0 0 0 : 1 1.00 1.00 0 1 1
3 GEN 13.8 0 0 0 : 2 1.05 1.01 2 1 1
4 OFF 13.8 0 0 0 : 2 1.00 1.00 0 1 1
load data
2 LOAD 138 1 : 1 50 10
generator data
3 GEN 13.8 1 : 1 0 0 0 0 0 0 0 40 80 10 5 60 -10 100
4 GEN 13.8 1 : 0 0 0 0 0 0 0 0 0 10 0 0 0 0 100
branch data
1 A 138 2 B 138 1 : 1 0.01 0.20 0.05 100 110 120
svd data
2 SVD 138 0 0 1 : 1 1 0 0 0 0 0 0 0.40 0 0 0.02 /
3 0.25 2 -0.5 4 0.5 0 1 1
5 SVD 138 0 0 1 : 0 1 0 0 0 0 0 0 0.80 0 0 0.02 /
2 0.40 1 0.20 0 0
transformer data
1 A 138 2 B 138 1 : 1 0 0 0 0.0 0.01 0.1
0 0 0
0 0 0
0 0 0
end
";

fn write_epc(contents: &str) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::NamedTempFile::with_suffix(".epc")?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    Ok(file)
}

#[test]
fn synthetic_epc_honors_type_impedance_and_svd_blocks() -> Result<()> {
    let epc = write_epc(SYNTHETIC_EPC)?;
    let path = epc.path().to_str().expect("utf-8 temp path");
    let network = parser::parse_epc(std::path::Path::new(path))?;

    assert_eq!(network.sbase, 100.0);
    let slack = network.buses.iter().find(|b| b.number == 1).unwrap();
    assert_eq!(slack.ty, 0);
    assert!((slack.volt - 1.02).abs() < 1e-9);
    assert!((slack.vsched - 1.01).abs() < 1e-9);

    let branch = network.branches.iter().find(|b| b.from_bus == 1).unwrap();
    assert!((branch.r - 0.01).abs() < 1e-12);
    assert!((branch.x - 0.20).abs() < 1e-12);
    assert!((branch.b - 0.05).abs() < 1e-12);

    let active = network.switched_shunts.iter().find(|s| s.bus == 2).unwrap();
    assert_eq!(active.bank_pairs, vec![(3, 0.25), (2, -0.5), (4, 0.5)]);

    let out = tempfile::NamedTempFile::with_suffix(".rpf")?;
    let out_path = out.path().to_str().unwrap();
    raptrix_pslf_rs::write_pslf_to_rpf(path, None, out_path)?;

    let metadata = rpf_file_metadata(out.path())?;
    assert_eq!(
        metadata.get("rpf_version").map(String::as_str),
        Some("v0.14.5")
    );
    assert_eq!(RPF_VERSION, "v0.14.5");

    let tables: BTreeMap<_, _> = read_rpf_tables(out.path())?.into_iter().collect();
    let buses = tables.get(TABLE_BUSES).unwrap();
    let bus_id = buses
        .column_by_name("bus_id")
        .unwrap()
        .as_any()
        .downcast_ref::<Int32Array>()
        .unwrap();
    let bus_type = buses.column_by_name("type").unwrap();
    let v_mag = buses
        .column_by_name("v_mag_set")
        .unwrap()
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();

    let mut types = BTreeMap::new();
    for i in 0..bus_id.len() {
        let token = dict_utf8(bus_type.as_ref(), i);
        types.insert(bus_id.value(i), (token.to_string(), v_mag.value(i)));
    }
    assert_eq!(types.get(&1).unwrap().0, BUS_TYPE_SLACK);
    assert!((types.get(&1).unwrap().1 - 1.02).abs() < 1e-9);
    assert_eq!(types.get(&2).unwrap().0, BUS_TYPE_PQ);
    assert_eq!(types.get(&3).unwrap().0, BUS_TYPE_PV);
    assert_eq!(types.get(&4).unwrap().0, BUS_TYPE_PQ);

    let branches = tables.get("branches").unwrap();
    let b_shunt = branches
        .column_by_name("b_shunt")
        .unwrap()
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    assert!((b_shunt.value(0) - 0.05).abs() < 1e-12);
    for name in ["g_from", "b_from", "g_to", "b_to"] {
        let col = branches.column_by_name(name).unwrap();
        assert_eq!(col.null_count(), col.len());
    }

    let gens = tables.get(TABLE_GENERATORS).unwrap();
    let gen_bus = gens
        .column_by_name("bus_id")
        .unwrap()
        .as_any()
        .downcast_ref::<Int32Array>()
        .unwrap();
    let q_min = gens
        .column_by_name("q_min_mvar")
        .unwrap()
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    let q_max = gens
        .column_by_name("q_max_mvar")
        .unwrap()
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    let online = (0..gen_bus.len()).find(|&i| gen_bus.value(i) == 3).unwrap();
    assert!((q_min.value(online) - (-10.0)).abs() < 1e-9);
    assert!((q_max.value(online) - 60.0).abs() < 1e-9);

    let shunts = tables.get(TABLE_SWITCHED_SHUNTS).unwrap();
    let shunt_bus = shunts
        .column_by_name("bus_id")
        .unwrap()
        .as_any()
        .downcast_ref::<Int32Array>()
        .unwrap();
    let b_init = shunts
        .column_by_name("b_init_pu")
        .unwrap()
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    let inactive = (0..shunt_bus.len())
        .find(|&i| shunt_bus.value(i) == 5)
        .unwrap();
    assert!(b_init.value(inactive).abs() < 1e-12);

    let banks = tables.get(TABLE_SWITCHED_SHUNT_BANKS).unwrap();
    let bank_b = banks
        .column_by_name("b_mvar")
        .unwrap()
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    let positive_steps = (0..bank_b.len()).filter(|&i| bank_b.value(i) > 0.0).count();
    assert_eq!(positive_steps, 3 + 4 + 2 + 1);
    assert!((0..bank_b.len()).all(|i| bank_b.value(i) > 0.0));

    let t3 = tables.get("transformers_3w").unwrap();
    assert_eq!(t3.num_rows(), 0);

    Ok(())
}

#[test]
fn transformer_mode_does_not_change_electrical_rows() -> Result<()> {
    let epc = write_epc(SYNTHETIC_EPC)?;
    let path = epc.path().to_str().unwrap();
    let native = tempfile::NamedTempFile::with_suffix(".rpf")?;
    let expanded = tempfile::NamedTempFile::with_suffix(".rpf")?;
    raptrix_pslf_rs::write_pslf_to_rpf_with_options(
        path,
        None,
        native.path().to_str().unwrap(),
        &raptrix_pslf_rs::ExportOptions {
            transformer_representation_mode: TransformerRepresentationMode::Native3W,
            ..Default::default()
        },
    )?;
    raptrix_pslf_rs::write_pslf_to_rpf_with_options(
        path,
        None,
        expanded.path().to_str().unwrap(),
        &raptrix_pslf_rs::ExportOptions {
            transformer_representation_mode: TransformerRepresentationMode::Expanded,
            ..Default::default()
        },
    )?;
    let native_meta = rpf_file_metadata(native.path())?;
    let expanded_meta = rpf_file_metadata(expanded.path())?;
    assert_eq!(
        native_meta
            .get("rpf.transformer_representation_mode")
            .map(String::as_str),
        Some("native_3w")
    );
    assert_eq!(
        expanded_meta
            .get("rpf.transformer_representation_mode")
            .map(String::as_str),
        Some("expanded")
    );
    for path in [native.path(), expanded.path()] {
        let tables: BTreeMap<_, _> = read_rpf_tables(path)?.into_iter().collect();
        assert_eq!(tables.get("transformers_3w").unwrap().num_rows(), 0);
        assert!(tables.get("transformers_2w").unwrap().num_rows() > 0);
    }
    Ok(())
}

#[test]
fn validate_rejects_an_epc_with_no_buses() -> Result<()> {
    let empty = write_epc("title\nsynthetic\nsolution parameters\nsbase 100\nend\n")?;
    let report = validate_pslf_epc(empty.path().to_str().unwrap())?;
    assert!(!report.is_clean());
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "EMPTY_BUSES")
    );

    let ok = write_epc(SYNTHETIC_EPC)?;
    let report = validate_pslf_epc(ok.path().to_str().unwrap())?;
    assert!(report.is_clean());
    Ok(())
}

fn dict_utf8(col: &dyn Array, i: usize) -> &str {
    use arrow::array::DictionaryArray;
    use arrow::datatypes::Int32Type;
    let dict = col
        .as_any()
        .downcast_ref::<DictionaryArray<Int32Type>>()
        .expect("dictionary");
    let values = dict
        .values()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    values.value(dict.key(i).unwrap())
}
