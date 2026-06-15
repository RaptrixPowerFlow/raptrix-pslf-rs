// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! `raptrix-pslf-rs` — High-performance GE PSLF (`.epc` + `.dyd`) →
//! Raptrix PowerFlow Interchange v0.12.2 converter.

pub mod export;
pub mod models;
pub mod mrid;
pub mod parser;
pub mod validation;

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use arrow::record_batch::RecordBatch;
use raptrix_cim_arrow::{
    METADATA_KEY_CASE_FINGERPRINT, METADATA_KEY_CASE_MODE, METADATA_KEY_DEFAULT_SHUNT_CONTROL_MODE,
    METADATA_KEY_SOLVED_STATE_PRESENCE, METADATA_KEY_VALIDATION_MODE, RootWriteOptions,
    TABLE_AREAS, TABLE_BRANCHES, TABLE_BUSES, TABLE_CONTINGENCIES, TABLE_DC_LINES_2W,
    TABLE_DYNAMICS_MODELS, TABLE_FIXED_SHUNTS, TABLE_GENERATORS, TABLE_INTERFACES, TABLE_LOADS,
    TABLE_METADATA, TABLE_MULTI_SECTION_LINES, TABLE_OWNERS, TABLE_SWITCHED_SHUNT_BANKS,
    TABLE_SWITCHED_SHUNTS, TABLE_TRANSFORMERS_2W, TABLE_TRANSFORMERS_3W, TABLE_ZONES,
    write_root_rpf_with_metadata,
};

pub use raptrix_cim_arrow::{
    RPF_VERSION, RpfSummary, TableSummary, read_rpf_tables, summarize_rpf, validate_rpf_file,
};

const METADATA_KEY_TRANSFORMER_REPRESENTATION_MODE: &str = "rpf.transformer_representation_mode";
const METADATA_KEY_LOADS_ZIP_FIDELITY_PRESENCE: &str = "rpf.loads.zip_fidelity_presence";

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
    /// Optional `scenario_context` rows (unsupported on the standard PSLF export path).
    pub scenario_context_rows: Vec<ScenarioContextRow>,
}

/// One row for the optional v0.9.0 `scenario_context` table when a writer emits that optional root.
#[derive(Debug, Clone)]
pub struct ScenarioContextRow {
    pub scenario_context_id: i32,
    pub case_id: String,
    pub source_type: String,
    pub priority: String,
    pub violation_type: Option<String>,
    pub nerc_recovery_status: Option<String>,
    pub recovery_time_min: Option<f64>,
    pub cleared_by_reserves: Option<bool>,
    pub planning_feedback_flag: bool,
    pub planning_assumption_violated: Option<String>,
    pub recommended_action: Option<String>,
    pub investigation_summary: Option<String>,
    pub load_forecast_error_pct: Option<f64>,
    pub created_timestamp_utc: String,
    pub params: Vec<(String, f64)>,
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
    if !options.scenario_context_rows.is_empty() {
        bail!(
            "scenario_context_rows is non-empty, but optional `scenario_context` root emission is unsupported in this build's Arrow IPC path. Omit scenario_context_rows for standard PSLF exports."
        );
    }

    let mut network = parser::parse_epc(std::path::Path::new(epc_path))
        .with_context(|| format!("failed to parse EPC file: {epc_path}"))?;

    if let Some(dyd) = dyd_path {
        parser::parse_dyd(std::path::Path::new(dyd), &mut network)
            .with_context(|| format!("failed to parse DYD file: {dyd}"))?;
    }

    export::prepare_network_for_export(&mut network);

    let sbase = if network.sbase.abs() > 1.0e-9 {
        network.sbase
    } else {
        100.0
    };

    let case_fingerprint = export::compute_case_fingerprint(&network);
    let case_mode = resolve_case_mode(&network, options)?;
    let ibr_subtype_by_gen = export::compute_ibr_subtype_by_generator(&network);
    let solved_state_presence = "not_computed";
    let mut generator_q_sanitization = export::GeneratorQSanitizationStats::default();

    let bus_nominal_kv = network
        .buses
        .iter()
        .map(|b| (b.number, b.kv))
        .collect::<HashMap<_, _>>();
    let agg_by_bus = export::build_bus_aggregates_for_export(&network);
    let star_leg_mrid_map = mrid::build_star_leg_mrid_map(&network.transformers_3w);

    let mut table_batches: HashMap<&'static str, RecordBatch> = HashMap::new();

    table_batches.insert(
        TABLE_METADATA,
        export::build_metadata_batch(
            &network,
            &case_fingerprint,
            &case_mode,
            solved_state_presence,
            &ibr_subtype_by_gen,
            options,
        )?,
    );
    table_batches.insert(
        TABLE_BUSES,
        export::build_buses_batch(&network.buses, &agg_by_bus)?,
    );
    table_batches.insert(
        TABLE_GENERATORS,
        export::build_generators_batch(
            &network.generators,
            &network.dyd_generators,
            &ibr_subtype_by_gen,
            &mut generator_q_sanitization,
        )?,
    );
    table_batches.insert(
        TABLE_LOADS,
        export::build_loads_batch(&network.loads, sbase)?,
    );
    table_batches.insert(
        TABLE_BRANCHES,
        export::build_branches_batch(&network.branches, &bus_nominal_kv, sbase)?,
    );
    table_batches.insert(
        TABLE_TRANSFORMERS_2W,
        export::build_transformers_2w_batch(
            &network.transformers,
            &bus_nominal_kv,
            sbase,
            &star_leg_mrid_map,
        )?,
    );
    table_batches.insert(
        TABLE_TRANSFORMERS_3W,
        if network.transformers_3w.is_empty() {
            export::empty_table(TABLE_TRANSFORMERS_3W)?
        } else {
            export::build_transformers_3w_batch(&network.transformers_3w, &bus_nominal_kv, sbase)?
        },
    );
    table_batches.insert(TABLE_AREAS, export::build_areas_batch(&network.areas)?);
    table_batches.insert(TABLE_ZONES, export::build_zones_batch(&network.zones)?);
    table_batches.insert(TABLE_OWNERS, export::build_owners_batch(&network.owners)?);
    table_batches.insert(
        TABLE_FIXED_SHUNTS,
        export::build_fixed_shunts_batch(&network.fixed_shunts, sbase)?,
    );
    table_batches.insert(
        TABLE_SWITCHED_SHUNTS,
        export::build_switched_shunts_batch(&network.switched_shunts, sbase)?,
    );
    table_batches.insert(
        TABLE_SWITCHED_SHUNT_BANKS,
        export::build_switched_shunt_banks_batch(&network.switched_shunt_banks)?,
    );
    table_batches.insert(
        TABLE_MULTI_SECTION_LINES,
        export::empty_table(TABLE_MULTI_SECTION_LINES)?,
    );
    table_batches.insert(TABLE_DC_LINES_2W, export::empty_table(TABLE_DC_LINES_2W)?);
    table_batches.insert(
        TABLE_CONTINGENCIES,
        export::empty_table(TABLE_CONTINGENCIES)?,
    );
    table_batches.insert(TABLE_INTERFACES, export::empty_table(TABLE_INTERFACES)?);

    let dynamics_batch = if network.dyd_models.is_empty() {
        export::empty_table(TABLE_DYNAMICS_MODELS)?
    } else {
        export::build_dynamics_models_batch(&network.dyd_models)?
    };
    table_batches.insert(TABLE_DYNAMICS_MODELS, dynamics_batch);

    export::validate_export_invariants(&table_batches, options.transformer_representation_mode)?;

    let root_opts = RootWriteOptions {
        dynamics_are_stub: network.dyd_models.is_empty(),
        contingencies_are_stub: true,
        ..RootWriteOptions::default()
    };

    let mut additional_root_metadata = HashMap::new();
    additional_root_metadata.insert(
        METADATA_KEY_CASE_FINGERPRINT.to_string(),
        case_fingerprint.clone(),
    );
    additional_root_metadata.insert(
        METADATA_KEY_VALIDATION_MODE.to_string(),
        "converter_export".to_string(),
    );
    additional_root_metadata.insert(METADATA_KEY_CASE_MODE.to_string(), case_mode.clone());
    if let Some(mode_str) = resolved_default_shunt_control_mode(
        case_mode.as_str(),
        options.default_shunt_control_mode_override.as_deref(),
    ) {
        additional_root_metadata.insert(
            METADATA_KEY_DEFAULT_SHUNT_CONTROL_MODE.to_string(),
            mode_str,
        );
    }
    additional_root_metadata.insert(
        METADATA_KEY_SOLVED_STATE_PRESENCE.to_string(),
        solved_state_presence.to_string(),
    );
    additional_root_metadata.insert(
        METADATA_KEY_TRANSFORMER_REPRESENTATION_MODE.to_string(),
        options
            .transformer_representation_mode
            .as_stable_str()
            .to_string(),
    );
    additional_root_metadata.insert(
        METADATA_KEY_LOADS_ZIP_FIDELITY_PRESENCE.to_string(),
        export::classify_loads_zip_fidelity_presence(&network.loads).to_string(),
    );

    write_root_rpf_with_metadata(
        output,
        &table_batches,
        &root_opts,
        &additional_root_metadata,
    )
    .with_context(|| format!("failed to write RPF file: {output}"))?;

    if !generator_q_sanitization.is_empty() {
        eprintln!(
            "[converter] sanitized generator Q-limits on export: \
             swapped (QB > QT)={}, clamped non-finite QB={}, clamped non-finite QT={}.",
            generator_q_sanitization.swapped_q_limits,
            generator_q_sanitization.clamped_nonfinite_q_min,
            generator_q_sanitization.clamped_nonfinite_q_max,
        );
    }

    eprintln!(
        "[raptrix-pslf-rs] wrote {} (buses={}, generators={}, loads={}, branches={}, xfmr2w={}, svd={})",
        output,
        network.buses.len(),
        network.generators.len(),
        network.loads.len(),
        network.branches.len(),
        network.transformers.len(),
        network.switched_shunts.len(),
    );

    Ok(())
}

/// Determine `case_mode` from EPC bus voltage state (mirrors psse-rs RAW detection).
fn detect_case_mode(network: &models::Network) -> &'static str {
    if network.buses.is_empty() {
        return "warm_start_planning";
    }
    let is_flat = network
        .buses
        .iter()
        .all(|b| (b.volt - 1.0).abs() < 1.0e-4 && b.angle.abs() < 1.0e-4);
    if is_flat {
        "flat_start_planning"
    } else {
        "warm_start_planning"
    }
}

fn resolve_case_mode(network: &models::Network, options: &ExportOptions) -> Result<String> {
    const ALLOWED: &[&str] = &[
        "flat_start_planning",
        "warm_start_planning",
        "solved_snapshot",
        "hour_ahead_advisory",
    ];
    if let Some(raw) = &options.case_mode_override {
        let token = raw.trim();
        if ALLOWED.contains(&token) {
            return Ok(token.to_string());
        }
        bail!(
            "invalid case_mode override '{raw}'; expected one of: {}",
            ALLOWED.join(", ")
        );
    }
    Ok(detect_case_mode(network).to_string())
}

/// v0.9.5+: align with psse-rs / raptrix-cim-arrow planning shunt handoff.
pub(crate) fn resolved_default_shunt_control_mode(
    case_mode: &str,
    override_opt: Option<&str>,
) -> Option<String> {
    if let Some(raw) = override_opt {
        let t = raw.trim();
        if t.is_empty() {
            return None;
        }
        return Some(t.to_string());
    }
    match case_mode {
        "flat_start_planning" | "warm_start_planning" | "hour_ahead_advisory" => {
            Some("planning_full".to_string())
        }
        _ => None,
    }
}

pub fn validate_pslf_epc(epc_path: &str) -> Result<validation::ValidationReport> {
    let _ = parser::parse_epc(std::path::Path::new(epc_path))
        .with_context(|| format!("failed to parse EPC for validation: {epc_path}"))?;

    Ok(validation::ValidationReport::default())
}
