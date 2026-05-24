// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `raptrix-pslf-rs` — High-performance GE PSLF (`.epc` + `.dyd`) →
//! Raptrix PowerFlow Interchange v0.10.0 converter.
//!
//! This crate is a faithful structural and stylistic sibling of
//! `raptrix-psse-rs`. See the README and `docs/pslf-mapping.md` for
//! architecture, fidelity rules, and usage.

pub mod models;
pub mod parser;
pub mod validation;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{
    BooleanArray, BooleanBuilder, Float64Builder, Int8Builder, Int32Builder, ListBuilder,
    MapBuilder, MapFieldNames, StringBuilder, StringDictionaryBuilder, new_null_array,
};
use arrow::datatypes::{Int32Type, UInt32Type};
use arrow::record_batch::RecordBatch;
use chrono::{SecondsFormat, Utc};
use raptrix_cim_arrow::{
    METADATA_KEY_COMPUTATIONAL_LOAD_MODE, RootWriteOptions, TABLE_AREAS, TABLE_BRANCHES,
    TABLE_BUSES, TABLE_CONTINGENCIES, TABLE_DC_LINES_2W, TABLE_DYNAMICS_MODELS, TABLE_FIXED_SHUNTS,
    TABLE_GENERATORS, TABLE_INTERFACES, TABLE_LOADS, TABLE_METADATA, TABLE_MULTI_SECTION_LINES,
    TABLE_OWNERS, TABLE_SWITCHED_SHUNT_BANKS, TABLE_SWITCHED_SHUNTS, TABLE_TRANSFORMERS_2W,
    TABLE_TRANSFORMERS_3W, TABLE_ZONES, table_schema, write_root_rpf_with_metadata,
};

use crate::models::{Bus, DydGeneratorData, Generator, Load, Network};

// Re-export reader utilities so tests and tools can use them directly.
pub use raptrix_cim_arrow::{
    RpfSummary, TableSummary, read_rpf_tables, summarize_rpf, validate_rpf_file,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformerRepresentationMode {
    Expanded,
    #[default]
    Native3W,
}

impl TransformerRepresentationMode {
    pub fn as_stable_str(self) -> &'static str {
        match self {
            TransformerRepresentationMode::Expanded => "expanded",
            TransformerRepresentationMode::Native3W => "native_3w",
        }
    }

    pub fn from_cli_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "expanded" => Some(TransformerRepresentationMode::Expanded),
            "native" | "native_3w" | "native-3w" => Some(TransformerRepresentationMode::Native3W),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    pub transformer_representation_mode: TransformerRepresentationMode,
    pub study_purpose: Option<String>,
    pub scenario_tags: Vec<String>,
    pub case_mode_override: Option<String>,
    pub default_shunt_control_mode_override: Option<String>,
}

pub fn write_pslf_to_rpf(epc_path: &str, dyd_path: Option<&str>, output: &str) -> Result<()> {
    write_pslf_to_rpf_with_options(epc_path, dyd_path, output, &ExportOptions::default())
}

pub fn write_pslf_to_rpf_with_options(
    epc_path: &str,
    dyd_path: Option<&str>,
    output: &str,
    options: &ExportOptions,
) -> Result<()> {
    let mut network = parser::parse_epc(std::path::Path::new(epc_path))
        .with_context(|| format!("failed to parse EPC file: {epc_path}"))?;

    if let Some(dyd) = dyd_path {
        parser::parse_dyd(std::path::Path::new(dyd), &mut network)
            .with_context(|| format!("failed to parse DYD file: {dyd}"))?;
    }

    let sbase = if network.sbase.abs() > 1.0e-9 {
        network.sbase
    } else {
        100.0
    };

    let case_fingerprint = compute_case_fingerprint(&network);
    let case_mode = options
        .case_mode_override
        .clone()
        .unwrap_or_else(|| "warm_start_planning".to_string());

    let mut table_batches: HashMap<&'static str, RecordBatch> = HashMap::new();

    table_batches.insert(
        TABLE_METADATA,
        build_metadata_batch(&network, &case_fingerprint, &case_mode, options)?,
    );
    table_batches.insert(TABLE_BUSES, build_buses_batch(&network.buses)?);
    table_batches.insert(
        TABLE_GENERATORS,
        build_generators_batch(&network.generators, &network.dyd_generators)?,
    );
    table_batches.insert(TABLE_LOADS, build_loads_batch(&network.loads, sbase)?);
    table_batches.insert(TABLE_BRANCHES, empty_table(TABLE_BRANCHES)?);
    table_batches.insert(TABLE_TRANSFORMERS_2W, empty_table(TABLE_TRANSFORMERS_2W)?);
    table_batches.insert(TABLE_TRANSFORMERS_3W, empty_table(TABLE_TRANSFORMERS_3W)?);
    table_batches.insert(TABLE_AREAS, empty_table(TABLE_AREAS)?);
    table_batches.insert(TABLE_ZONES, empty_table(TABLE_ZONES)?);
    table_batches.insert(TABLE_OWNERS, empty_table(TABLE_OWNERS)?);
    table_batches.insert(TABLE_FIXED_SHUNTS, empty_table(TABLE_FIXED_SHUNTS)?);
    table_batches.insert(TABLE_SWITCHED_SHUNTS, empty_table(TABLE_SWITCHED_SHUNTS)?);
    table_batches.insert(
        TABLE_SWITCHED_SHUNT_BANKS,
        empty_table(TABLE_SWITCHED_SHUNT_BANKS)?,
    );
    table_batches.insert(
        TABLE_MULTI_SECTION_LINES,
        empty_table(TABLE_MULTI_SECTION_LINES)?,
    );
    table_batches.insert(TABLE_DC_LINES_2W, empty_table(TABLE_DC_LINES_2W)?);
    table_batches.insert(TABLE_CONTINGENCIES, empty_table(TABLE_CONTINGENCIES)?);
    table_batches.insert(TABLE_INTERFACES, empty_table(TABLE_INTERFACES)?);
    let dynamics_batch = if network.dyd_models.is_empty() {
        empty_table(TABLE_DYNAMICS_MODELS)?
    } else {
        empty_table(TABLE_DYNAMICS_MODELS)?
    };
    table_batches.insert(TABLE_DYNAMICS_MODELS, dynamics_batch);

    let root_opts = RootWriteOptions {
        dynamics_are_stub: network.dyd_models.is_empty(),
        contingencies_are_stub: true,
        ..RootWriteOptions::default()
    };

    write_root_rpf_with_metadata(output, &table_batches, &root_opts, &HashMap::new())
        .with_context(|| format!("failed to write RPF file: {output}"))?;

    eprintln!(
        "[raptrix-pslf-rs] wrote {} (buses={}, generators={}, loads={})",
        output,
        network.buses.len(),
        network.generators.len(),
        network.loads.len()
    );

    Ok(())
}

pub fn validate_pslf_epc(epc_path: &str) -> Result<validation::ValidationReport> {
    let _ = parser::parse_epc(std::path::Path::new(epc_path))
        .with_context(|| format!("failed to parse EPC for validation: {epc_path}"))?;

    Ok(validation::ValidationReport::default())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_table(name: &'static str) -> Result<RecordBatch> {
    let schema =
        table_schema(name).ok_or_else(|| anyhow::anyhow!("unknown canonical table: {name}"))?;
    Ok(RecordBatch::new_empty(Arc::new(schema)))
}

fn compute_case_fingerprint(network: &Network) -> String {
    format!(
        "pslf:{}:{}:{}:{}",
        network.title.len(),
        network.buses.len(),
        network.generators.len(),
        network.loads.len()
    )
}

fn infer_study_purpose(title: &str) -> Option<String> {
    let t = title.to_ascii_lowercase();
    if t.contains("planning") || t.contains("2030") || t.contains("future") {
        return Some("planning".to_string());
    }
    if t.contains("onpeak") || t.contains("offpeak") || t.contains("operations") {
        return Some("operations".to_string());
    }
    None
}

fn infer_scenario_tags(title: &str) -> Vec<String> {
    let t = title.to_ascii_lowercase();
    let mut tags = Vec::new();
    for (needle, tag) in [
        ("onpeak", "onpeak"),
        ("offpeak", "offpeak"),
        ("summerpeak", "summer_peak"),
        ("winter", "winter"),
        ("dynamic", "dynamic"),
        ("static", "static"),
        ("gfm", "gfm"),
        ("ibr", "ibr"),
    ] {
        if t.contains(needle) {
            tags.push(tag.to_string());
        }
    }
    tags
}

fn generator_is_ibr(generator: &Generator, dyd_generators: &[DydGeneratorData]) -> bool {
    dyd_generators
        .iter()
        .any(|dg| dg.bus_id == generator.bus && dg.is_ibr)
}

// ---------------------------------------------------------------------------
// Table builders
// ---------------------------------------------------------------------------

fn build_metadata_batch(
    network: &Network,
    case_fingerprint_value: &str,
    case_mode: &str,
    options: &ExportOptions,
) -> Result<RecordBatch> {
    let now_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let schema = Arc::new(table_schema(TABLE_METADATA).expect("metadata schema must exist"));

    let base_mva = arrow::array::Float64Array::from(vec![network.sbase]);
    let frequency_hz = arrow::array::Float64Array::from(vec![60.0]);
    let psse_version = arrow::array::Int32Array::from(vec![0]);
    let is_planning_case = arrow::array::BooleanArray::from(vec![false]);

    let mut study_name = StringDictionaryBuilder::<Int32Type>::new();
    study_name.append_value(network.title.as_ref());
    let mut source_case_id = StringDictionaryBuilder::<Int32Type>::new();
    source_case_id.append_value(network.title.as_ref());
    let mut validation_mode = StringDictionaryBuilder::<Int32Type>::new();
    validation_mode.append_value("converter_export");

    let mut timestamp_utc = StringBuilder::new();
    timestamp_utc.append_value(now_utc.as_str());
    let mut snapshot_timestamp_utc = StringBuilder::new();
    snapshot_timestamp_utc.append_value(now_utc.as_str());
    let mut raptrix_version = StringBuilder::new();
    raptrix_version.append_value(env!("CARGO_PKG_VERSION"));
    let mut case_fingerprint = StringBuilder::new();
    case_fingerprint.append_value(case_fingerprint_value);

    let custom_meta_type = schema
        .field_with_name("custom_metadata")
        .expect("custom_metadata field must exist in metadata schema")
        .data_type()
        .clone();
    let custom_metadata = new_null_array(&custom_meta_type, 1);

    let mut case_mode_arr = StringDictionaryBuilder::<Int32Type>::new();
    case_mode_arr.append_value(case_mode);

    let mut solved_state_presence_arr = StringDictionaryBuilder::<Int32Type>::new();
    solved_state_presence_arr.append_value("not_computed");

    let mut solver_version_arr = StringBuilder::new();
    solver_version_arr.append_null();
    let mut solver_iterations_arr = Int32Builder::new();
    solver_iterations_arr.append_null();
    let mut solver_accuracy_arr = Float64Builder::new();
    solver_accuracy_arr.append_null();
    let mut solver_mode_arr = StringDictionaryBuilder::<Int32Type>::new();
    solver_mode_arr.append_null();

    let mut slack_bus_id_solved_arr = Int32Builder::new();
    slack_bus_id_solved_arr.append_null();
    let mut angle_reference_deg_arr = Float64Builder::new();
    angle_reference_deg_arr.append_null();
    let mut solved_shunt_state_presence_arr = StringDictionaryBuilder::<Int32Type>::new();
    solved_shunt_state_presence_arr.append_null();

    let has_ibr_value = network.dyd_generators.iter().any(|dg| dg.is_ibr);
    let modern_grid_profile_value = has_ibr_value;

    let total_pmax_mw: f64 = network
        .generators
        .iter()
        .filter(|g| g.status != 0)
        .map(|g| g.pt.max(0.0))
        .sum();
    let ibr_pmax_mw: f64 = network
        .generators
        .iter()
        .filter(|g| g.status != 0)
        .filter(|g| generator_is_ibr(g, &network.dyd_generators))
        .map(|g| g.pt.max(0.0))
        .sum();
    let mut ibr_penetration_pct_arr = Float64Builder::new();
    if total_pmax_mw > 1.0e-9 {
        ibr_penetration_pct_arr.append_value((ibr_pmax_mw / total_pmax_mw) * 100.0);
    } else {
        ibr_penetration_pct_arr.append_null();
    }

    let study_purpose_value = options
        .study_purpose
        .clone()
        .or_else(|| infer_study_purpose(network.title.as_ref()));
    let mut study_purpose_arr = StringBuilder::new();
    study_purpose_arr.append_option(study_purpose_value.as_deref());

    let scenario_tags_value = if options.scenario_tags.is_empty() {
        infer_scenario_tags(network.title.as_ref())
    } else {
        options.scenario_tags.clone()
    };
    let scenario_item_field = Arc::new(arrow::datatypes::Field::new(
        "item",
        arrow::datatypes::DataType::Utf8,
        false,
    ));
    let mut scenario_tags_arr =
        ListBuilder::new(StringBuilder::new()).with_field(scenario_item_field);
    if scenario_tags_value.is_empty() {
        scenario_tags_arr.append(false);
    } else {
        for tag in &scenario_tags_value {
            scenario_tags_arr.values().append_value(tag);
        }
        scenario_tags_arr.append(true);
    }

    let mut hour_ahead_uncertainty_band = Float64Builder::new();
    hour_ahead_uncertainty_band.append_null();
    let mut commitment_source = StringBuilder::new();
    commitment_source.append_null();
    let mut solver_q_limit_infeasible_count = Int32Builder::new();
    solver_q_limit_infeasible_count.append_null();
    let mut pv_to_pq_switch_count = Int32Builder::new();
    pv_to_pq_switch_count.append_null();
    let mut real_time_discovery = BooleanBuilder::new();
    real_time_discovery.append_null();

    let mut default_shunt_control_mode = StringDictionaryBuilder::<Int32Type>::new();
    if let Some(mode) = &options.default_shunt_control_mode_override {
        default_shunt_control_mode.append_value(mode.as_str());
    } else {
        default_shunt_control_mode.append_null();
    }

    let computational_load_mode_type = schema
        .field_with_name(METADATA_KEY_COMPUTATIONAL_LOAD_MODE)
        .expect("metadata schema must include computational_load_mode")
        .data_type()
        .clone();
    let computational_load_mode_col = new_null_array(&computational_load_mode_type, 1);

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(base_mva),
            Arc::new(frequency_hz),
            Arc::new(psse_version),
            Arc::new(study_name.finish()),
            Arc::new(timestamp_utc.finish()),
            Arc::new(raptrix_version.finish()),
            Arc::new(is_planning_case),
            Arc::new(source_case_id.finish()),
            Arc::new(snapshot_timestamp_utc.finish()),
            Arc::new(case_fingerprint.finish()),
            Arc::new(validation_mode.finish()),
            custom_metadata,
            Arc::new(case_mode_arr.finish()),
            Arc::new(solved_state_presence_arr.finish()),
            Arc::new(solver_version_arr.finish()),
            Arc::new(solver_iterations_arr.finish()),
            Arc::new(solver_accuracy_arr.finish()),
            Arc::new(solver_mode_arr.finish()),
            Arc::new(slack_bus_id_solved_arr.finish()),
            Arc::new(angle_reference_deg_arr.finish()),
            Arc::new(solved_shunt_state_presence_arr.finish()),
            Arc::new(BooleanArray::from(vec![modern_grid_profile_value])),
            Arc::new(ibr_penetration_pct_arr.finish()),
            Arc::new(BooleanArray::from(vec![has_ibr_value])),
            Arc::new(BooleanArray::from(vec![false])),
            Arc::new(BooleanArray::from(vec![false])),
            Arc::new(study_purpose_arr.finish()),
            Arc::new(scenario_tags_arr.finish()),
            Arc::new(hour_ahead_uncertainty_band.finish()),
            Arc::new(commitment_source.finish()),
            Arc::new(solver_q_limit_infeasible_count.finish()),
            Arc::new(pv_to_pq_switch_count.finish()),
            Arc::new(real_time_discovery.finish()),
            Arc::new(default_shunt_control_mode.finish()),
            computational_load_mode_col,
        ],
    )
    .context("building metadata batch")
}

fn build_buses_batch(buses: &[Bus]) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_BUSES).expect("buses schema must exist"));

    let mut bus_id = Int32Builder::new();
    let mut name = StringDictionaryBuilder::<Int32Type>::new();
    let mut bus_type = Int8Builder::new();
    let mut p_sched = Float64Builder::new();
    let mut q_sched = Float64Builder::new();
    let mut v_mag_set = Float64Builder::new();
    let mut v_ang_set = Float64Builder::new();
    let mut q_min = Float64Builder::new();
    let mut q_max = Float64Builder::new();
    let mut g_shunt = Float64Builder::new();
    let mut b_shunt = Float64Builder::new();
    let mut area = Int32Builder::new();
    let mut zone = Int32Builder::new();
    let mut owner = Int32Builder::new();
    let mut v_min = Float64Builder::new();
    let mut v_max = Float64Builder::new();
    let mut p_min_agg = Float64Builder::new();
    let mut p_max_agg = Float64Builder::new();
    let mut nominal_kv = Float64Builder::new();
    let mut bus_uuid = StringDictionaryBuilder::<Int32Type>::new();
    let mut qd_load_pu = Float64Builder::new();
    let mut qg_sched_pu = Float64Builder::new();

    for bus in buses {
        bus_id.append_value(bus.number as i32);
        name.append_value(bus.name.as_ref());
        bus_type.append_value(bus.ty as i8);
        p_sched.append_value(0.0);
        q_sched.append_value(0.0);
        v_mag_set.append_value(bus.volt);
        v_ang_set.append_value(bus.angle.to_radians());
        q_min.append_value(-9999.0);
        q_max.append_value(9999.0);
        g_shunt.append_value(0.0);
        b_shunt.append_value(0.0);
        area.append_value(bus.area as i32);
        zone.append_value(bus.zone as i32);
        owner.append_null();
        v_min.append_value(0.9);
        v_max.append_value(1.1);
        p_min_agg.append_value(-9999.0);
        p_max_agg.append_value(9999.0);
        nominal_kv.append_value(bus.kv);
        bus_uuid.append_value(format!("pslf:bus:{}", bus.number));
        qd_load_pu.append_value(0.0);
        qg_sched_pu.append_value(0.0);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(bus_id.finish()),
            Arc::new(name.finish()),
            Arc::new(bus_type.finish()),
            Arc::new(p_sched.finish()),
            Arc::new(q_sched.finish()),
            Arc::new(v_mag_set.finish()),
            Arc::new(v_ang_set.finish()),
            Arc::new(q_min.finish()),
            Arc::new(q_max.finish()),
            Arc::new(g_shunt.finish()),
            Arc::new(b_shunt.finish()),
            Arc::new(area.finish()),
            Arc::new(zone.finish()),
            Arc::new(owner.finish()),
            Arc::new(v_min.finish()),
            Arc::new(v_max.finish()),
            Arc::new(p_min_agg.finish()),
            Arc::new(p_max_agg.finish()),
            Arc::new(nominal_kv.finish()),
            Arc::new(bus_uuid.finish()),
            Arc::new(qd_load_pu.finish()),
            Arc::new(qg_sched_pu.finish()),
        ],
    )
    .context("building buses batch")
}

fn build_generators_batch(
    generators: &[Generator],
    dyd_generators: &[DydGeneratorData],
) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_GENERATORS).expect("generators schema must exist"));

    let map_field_names = MapFieldNames {
        entry: "entries".to_string(),
        key: "key".to_string(),
        value: "value".to_string(),
    };

    let mut generator_id = Int32Builder::new();
    let mut bus_id = Int32Builder::new();
    let mut name_b = StringBuilder::new();
    let mut unit_type = StringBuilder::new();
    let mut hierarchy_level = StringBuilder::new();
    let mut parent_generator_id = Int32Builder::new();
    let mut aggregation_count = Int32Builder::new();
    let mut status = BooleanBuilder::new();
    let mut is_ibr = BooleanBuilder::new();
    let mut ibr_subtype = StringBuilder::new();
    let mut p_sched_mw = Float64Builder::new();
    let mut q_sched_mvar = Float64Builder::new();
    let mut p_min_mw = Float64Builder::new();
    let mut p_max_mw = Float64Builder::new();
    let mut q_min_mvar = Float64Builder::new();
    let mut q_max_mvar = Float64Builder::new();
    let mut mbase_mva = Float64Builder::new();
    let mut uol_mw = Float64Builder::new();
    let mut lol_mw = Float64Builder::new();
    let mut ramp_rate_up_mw_min = Float64Builder::new();
    let mut ramp_rate_down_mw_min = Float64Builder::new();
    let mut owner_id = Int32Builder::new();
    let mut market_resource_id = StringBuilder::new();
    let mut controlled_bus_id = Int32Builder::new();
    let mut params = MapBuilder::new(
        Some(map_field_names),
        StringBuilder::new(),
        Float64Builder::new(),
    );

    for (idx, generator) in generators.iter().enumerate() {
        let ibr = generator_is_ibr(generator, dyd_generators);

        generator_id.append_value((idx + 1) as i32);
        bus_id.append_value(generator.bus as i32);
        name_b.append_null();
        unit_type.append_value("unit");
        hierarchy_level.append_value("unit");
        parent_generator_id.append_null();
        aggregation_count.append_null();
        status.append_value(generator.status != 0);
        is_ibr.append_value(ibr);
        if ibr {
            ibr_subtype.append_value("generic_ibr");
        } else {
            ibr_subtype.append_null();
        }
        p_sched_mw.append_value(generator.pg);
        q_sched_mvar.append_value(generator.qg);
        p_min_mw.append_value(generator.pb);
        p_max_mw.append_value(generator.pt);
        q_min_mvar.append_value(-9999.0);
        q_max_mvar.append_value(9999.0);
        mbase_mva.append_value(generator.mbase);
        uol_mw.append_null();
        lol_mw.append_null();
        ramp_rate_up_mw_min.append_null();
        ramp_rate_down_mw_min.append_null();
        owner_id.append_null();
        market_resource_id.append_null();
        controlled_bus_id.append_value(0);
        params.append(false).context("building generators.params null entry")?;
    }

    let params_arr = params.finish();
    let params_target_type = schema
        .field_with_name("params")
        .expect("params field must exist in generators schema")
        .data_type()
        .clone();
    let params_cast = arrow::compute::cast(&params_arr, &params_target_type)
        .context("casting generators params")?;

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(generator_id.finish()),
            Arc::new(bus_id.finish()),
            Arc::new(name_b.finish()),
            Arc::new(unit_type.finish()),
            Arc::new(hierarchy_level.finish()),
            Arc::new(parent_generator_id.finish()),
            Arc::new(aggregation_count.finish()),
            Arc::new(status.finish()),
            Arc::new(is_ibr.finish()),
            Arc::new(ibr_subtype.finish()),
            Arc::new(p_sched_mw.finish()),
            Arc::new(q_sched_mvar.finish()),
            Arc::new(p_min_mw.finish()),
            Arc::new(p_max_mw.finish()),
            Arc::new(q_min_mvar.finish()),
            Arc::new(q_max_mvar.finish()),
            Arc::new(mbase_mva.finish()),
            Arc::new(uol_mw.finish()),
            Arc::new(lol_mw.finish()),
            Arc::new(ramp_rate_up_mw_min.finish()),
            Arc::new(ramp_rate_down_mw_min.finish()),
            Arc::new(owner_id.finish()),
            Arc::new(market_resource_id.finish()),
            params_cast,
            Arc::new(controlled_bus_id.finish()),
        ],
    )
    .context("building generators batch")
}

fn build_loads_batch(loads: &[Load], base_mva: f64) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_LOADS).expect("loads schema must exist"));

    let mut bus_id = Int32Builder::new();
    let mut id = StringDictionaryBuilder::<Int32Type>::new();
    let mut status = BooleanBuilder::new();
    let mut p_pu = Float64Builder::new();
    let mut q_pu = Float64Builder::new();
    let mut p_i_pu = Float64Builder::new();
    let mut q_i_pu = Float64Builder::new();
    let mut p_y_pu = Float64Builder::new();
    let mut q_y_pu = Float64Builder::new();
    let mut name_b = StringDictionaryBuilder::<UInt32Type>::new();

    for load in loads {
        bus_id.append_value(load.bus as i32);
        id.append_value(load.id.as_ref());
        status.append_value(load.status != 0);
        p_pu.append_value(load.p / base_mva);
        q_pu.append_value(load.q / base_mva);
        p_i_pu.append_null();
        q_i_pu.append_null();
        p_y_pu.append_null();
        q_y_pu.append_null();
        name_b.append_null();
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(bus_id.finish()),
            Arc::new(id.finish()),
            Arc::new(status.finish()),
            Arc::new(p_pu.finish()),
            Arc::new(q_pu.finish()),
            Arc::new(p_i_pu.finish()),
            Arc::new(q_i_pu.finish()),
            Arc::new(p_y_pu.finish()),
            Arc::new(q_y_pu.finish()),
            Arc::new(name_b.finish()),
        ],
    )
    .context("building loads batch")
}
