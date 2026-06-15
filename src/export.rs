// raptrix-pslf-rs — RPF export builders (mirrors raptrix-psse-rs export layer).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use arrow::array::{
    BooleanArray, BooleanBuilder, Float64Array, Float64Builder, Int8Builder, Int32Array,
    Int32Builder, ListBuilder, MapBuilder, MapFieldNames, StringBuilder, StringDictionaryBuilder,
    new_null_array,
};
use arrow::datatypes::{Int32Type, UInt32Type};
use arrow::record_batch::RecordBatch;
use chrono::{SecondsFormat, Utc};
use raptrix_cim_arrow::{
    METADATA_KEY_COMPUTATIONAL_LOAD_MODE, TABLE_AREAS, TABLE_BRANCHES, TABLE_BUSES,
    TABLE_DYNAMICS_MODELS, TABLE_FIXED_SHUNTS, TABLE_GENERATORS, TABLE_LOADS, TABLE_METADATA,
    TABLE_OWNERS, TABLE_SWITCHED_SHUNT_BANKS, TABLE_SWITCHED_SHUNTS, TABLE_TRANSFORMERS_2W,
    TABLE_TRANSFORMERS_3W, TABLE_ZONES, table_schema,
};

use crate::models::{
    Area, Branch, Bus, DydGeneratorData, DydModelData, FixedShunt, Generator, Load, Network, Owner,
    SwitchedShunt, SwitchedShuntBankRow, Transformer2W, Transformer3W, Zone,
};
use crate::mrid::{
    self, synth_branch_mrid, synth_generator_mrid, synth_transformer_2w_mrid_with_star_legs,
    synth_transformer_3w_mrid,
};
use crate::{ExportOptions, TransformerRepresentationMode};

#[derive(Debug, Clone, Copy, Default)]
pub struct GeneratorQSanitizationStats {
    pub swapped_q_limits: usize,
    pub clamped_nonfinite_q_min: usize,
    pub clamped_nonfinite_q_max: usize,
}

impl GeneratorQSanitizationStats {
    pub fn is_empty(self) -> bool {
        self.swapped_q_limits == 0
            && self.clamped_nonfinite_q_min == 0
            && self.clamped_nonfinite_q_max == 0
    }
}

pub fn empty_table(name: &'static str) -> Result<RecordBatch> {
    let schema =
        table_schema(name).ok_or_else(|| anyhow::anyhow!("unknown canonical table: {name}"))?;
    Ok(RecordBatch::new_empty(Arc::new(schema)))
}

pub fn compute_case_fingerprint(network: &Network) -> String {
    format!(
        "pslf:{}:{}:{}:{}",
        network.title.len(),
        network.buses.len(),
        network.generators.len(),
        network.loads.len()
    )
}

pub fn derive_switched_shunt_banks(network: &mut Network) {
    let base_mva = network.sbase.abs().max(1.0e-9);
    network.switched_shunt_banks.clear();
    for (shunt_row_idx, shunt) in network.switched_shunts.iter().enumerate() {
        let shunt_id = (shunt_row_idx + 1) as i32;
        for (bank_idx, (n_steps, b_pu)) in shunt.bank_pairs.iter().enumerate() {
            let bank_id = (bank_idx + 1) as i32;
            for step in 1..=(*n_steps as i32) {
                network.switched_shunt_banks.push(SwitchedShuntBankRow {
                    shunt_id,
                    bank_id,
                    b_mvar: *b_pu * base_mva,
                    status: shunt.status != 0,
                    step,
                });
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct BusAggregate {
    p_sched: f64,
    q_sched: f64,
    q_min: f64,
    q_max: f64,
    g_shunt: f64,
    b_shunt: f64,
    p_min_agg: f64,
    p_max_agg: f64,
    qd_load_pu: f64,
    qg_sched_pu: f64,
    has_generator: bool,
}

/// raptrix-core treats non-PSS/E branch/transformer R/X/B as physical (Ω, S) when
/// `from_nominal_kv` is set and converts with Z_base = V²/S_base. PSLF EPC stores
/// the same per-unit values as PSS/E RAW, so export must scale into physical units.
fn impedance_z_base(nominal_kv: f64, base_mva: f64) -> f64 {
    if nominal_kv > 1.0 {
        (nominal_kv * nominal_kv) / base_mva.abs().max(1.0e-9)
    } else {
        1.0
    }
}

fn branch_z_base(from_bus: u32, bus_nominal_kv: &HashMap<u32, f64>, base_mva: f64) -> f64 {
    let v_nom = bus_nominal_kv.get(&from_bus).copied().unwrap_or(0.0);
    impedance_z_base(v_nom, base_mva)
}

fn transformer_z_base(from_kv: f64, to_kv: f64, base_mva: f64) -> f64 {
    impedance_z_base(from_kv.max(to_kv), base_mva)
}

fn build_bus_aggregates(network: &Network) -> HashMap<u32, BusAggregate> {
    let base_mva = network.sbase.abs().max(1.0e-9);
    let mut agg_by_bus = HashMap::with_capacity(network.buses.len());

    for bus in &network.buses {
        let mut agg = BusAggregate {
            q_min: -9999.0,
            q_max: 9999.0,
            p_max_agg: 9999.0,
            ..Default::default()
        };
        if bus.ty == 1 {
            agg.q_min = -9999.0;
            agg.q_max = 9999.0;
            agg.p_max_agg = 9999.0;
        }
        agg_by_bus.insert(bus.number, agg);
    }

    for shunt in &network.fixed_shunts {
        if shunt.status == 0 {
            continue;
        }
        if let Some(agg) = agg_by_bus.get_mut(&shunt.bus) {
            agg.g_shunt += shunt.g / base_mva;
            agg.b_shunt += shunt.b / base_mva;
        }
    }

    for load in &network.loads {
        if load.status == 0 {
            continue;
        }
        if let Some(agg) = agg_by_bus.get_mut(&load.bus) {
            agg.p_sched -= load.p / base_mva;
            agg.q_sched -= load.q / base_mva;
            agg.qd_load_pu += load.q / base_mva;
        }
    }

    for generator in &network.generators {
        if generator.status == 0 {
            continue;
        }
        if let Some(agg) = agg_by_bus.get_mut(&generator.bus) {
            agg.p_sched += generator.pg / base_mva;
            agg.q_sched += generator.qg / base_mva;
            agg.qg_sched_pu += generator.qg / base_mva;

            if let Some((raw_qmin, raw_qmax)) = generator_q_limits_mvar(generator) {
                let qmin = raw_qmin / base_mva;
                let qmax = raw_qmax / base_mva;
                if agg.has_generator {
                    agg.q_min = agg.q_min.min(qmin);
                    agg.q_max = agg.q_max.max(qmax);
                } else {
                    agg.q_min = qmin;
                    agg.q_max = qmax;
                    agg.has_generator = true;
                }
            }

            agg.p_min_agg += generator.pb / base_mva;
            agg.p_max_agg += generator.pt / base_mva;
        }
    }

    agg_by_bus
}

fn canonical_bus_type_code(ty: u8) -> i8 {
    match ty {
        3 => 3,
        2 => 2,
        _ => 1,
    }
}

fn sanitize_bus_voltage(raw_vm: f64, raw_va_deg: f64) -> (f64, f64) {
    let v_mag = if raw_vm.is_finite() && raw_vm > 0.0 {
        raw_vm
    } else {
        1.0
    };
    let v_ang_rad = if raw_va_deg.is_finite() {
        raw_va_deg.to_radians()
    } else {
        0.0
    };
    (v_mag, v_ang_rad)
}

/// Raw QB/QT pair in MVAr from a generator record (swapped if inverted).
fn generator_q_limits_mvar(generator: &crate::models::Generator) -> Option<(f64, f64)> {
    let (raw_qmin, raw_qmax) = if generator.qb.is_finite() && generator.qt.is_finite() {
        if generator.qb > generator.qt {
            (generator.qt, generator.qb)
        } else {
            (generator.qb, generator.qt)
        }
    } else {
        let qmin = if generator.qb.is_finite() {
            generator.qb
        } else {
            0.0
        };
        let qmax = if generator.qt.is_finite() {
            generator.qt
        } else {
            0.0
        };
        (qmin, qmax)
    };
    if raw_qmin == 0.0 && raw_qmax == 0.0 {
        return None;
    }
    Some((raw_qmin, raw_qmax))
}

fn sanitize_generator_q_limits(
    raw_q_min: f64,
    raw_q_max: f64,
    stats: &mut GeneratorQSanitizationStats,
) -> (f64, f64) {
    let qmin = if raw_q_min.is_finite() {
        raw_q_min
    } else {
        stats.clamped_nonfinite_q_min += 1;
        0.0
    };
    let qmax = if raw_q_max.is_finite() {
        raw_q_max
    } else {
        stats.clamped_nonfinite_q_max += 1;
        0.0
    };
    if qmin > qmax {
        stats.swapped_q_limits += 1;
        (qmax, qmin)
    } else {
        (qmin, qmax)
    }
}

fn append_pslf_generator_raw_params(
    params: &mut MapBuilder<StringBuilder, Float64Builder>,
    machine: &Generator,
) -> Result<()> {
    let mut push = |k: &str, v: f64| -> Result<()> {
        params.keys().append_value(k);
        params.values().append_value(v);
        Ok(())
    };
    push("vs", machine.vs)?;
    if machine.ireg > 0 {
        push("ireg", machine.ireg as f64)?;
    }
    push("qg", machine.qg)?;
    Ok(())
}

fn canonical_ibr_subtype(model_family: &str) -> &'static str {
    let fam = model_family.to_ascii_lowercase();
    if fam.contains("bess") || fam.contains("battery") {
        "battery"
    } else if fam.contains("solar") || fam.contains("pv") {
        "solar"
    } else if fam.contains("wind") {
        "wind"
    } else {
        "generic_ibr"
    }
}

pub fn compute_ibr_subtype_by_generator(network: &Network) -> HashMap<(u32, String), String> {
    let mut out = HashMap::new();
    for generator in &network.generators {
        if generator.status == 0 {
            continue;
        }
        let key = (generator.bus, generator.id.to_string());
        if let Some(dg) = network
            .dyd_generators
            .iter()
            .find(|dg| dg.bus_id == generator.bus && dg.id.as_ref() == generator.id.as_ref())
        {
            if dg.is_ibr {
                out.insert(
                    key,
                    canonical_ibr_subtype(dg.model_family.as_ref()).to_string(),
                );
            }
        }
    }
    out
}

pub fn classify_loads_zip_fidelity_presence(_loads: &[Load]) -> &'static str {
    "not_available"
}

fn resolve_required_branch_nominal_kv(
    primary_bus_id: u32,
    opposite_bus_id: u32,
    bus_nominal_kv: &HashMap<u32, f64>,
    field: &str,
) -> Result<f64> {
    if let Some(kv) = bus_nominal_kv.get(&primary_bus_id).copied() {
        return Ok(kv);
    }
    if let Some(kv) = bus_nominal_kv.get(&opposite_bus_id).copied() {
        return Ok(kv);
    }
    bail!(
        "schema requires non-null {field}; no valid nominal kV found for bus {primary_bus_id} or opposite bus {opposite_bus_id}"
    );
}

fn resolve_required_transformer_nominal_kv(
    declared_nominal_kv: f64,
    primary_bus_id: u32,
    opposite_bus_id: u32,
    bus_nominal_kv: &HashMap<u32, f64>,
    field: &str,
) -> Result<f64> {
    if declared_nominal_kv > 0.0 {
        return Ok(declared_nominal_kv);
    }
    if let Some(kv) = bus_nominal_kv.get(&primary_bus_id).copied() {
        return Ok(kv);
    }
    if let Some(kv) = bus_nominal_kv.get(&opposite_bus_id).copied() {
        return Ok(kv);
    }
    bail!(
        "schema requires non-null {field}; no valid nominal kV found for bus {primary_bus_id} or opposite bus {opposite_bus_id}"
    );
}

fn generator_controlled_bus_id(machine: &Generator) -> i32 {
    let bus = machine.bus as i32;
    let ireg = machine.ireg as i32;
    if ireg == 0 || ireg == bus { 0 } else { ireg }
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

pub fn build_metadata_batch(
    network: &Network,
    case_fingerprint_value: &str,
    case_mode: &str,
    solved_state_presence: &str,
    ibr_subtype_by_gen: &HashMap<(u32, String), String>,
    options: &ExportOptions,
) -> Result<RecordBatch> {
    let now_utc = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let schema = Arc::new(table_schema(TABLE_METADATA).expect("metadata schema must exist"));

    let base_mva = arrow::array::Float64Array::from(vec![network.sbase]);
    let frequency_hz = arrow::array::Float64Array::from(vec![60.0]);
    let psse_version = arrow::array::Int32Array::from(vec![0]);
    let is_planning_case = arrow::array::BooleanArray::from(vec![true]);

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
    solved_state_presence_arr.append_value(solved_state_presence);

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

    let has_ibr_value = !ibr_subtype_by_gen.is_empty();
    let has_smart_valve_value = false;
    let has_multi_terminal_dc_value = !network.dc_converters.is_empty();
    let modern_grid_profile_value = has_ibr_value
        || has_smart_valve_value
        || has_multi_terminal_dc_value
        || !network.dc_buses.is_empty();

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
        .filter(|g| ibr_subtype_by_gen.contains_key(&(g.bus, g.id.to_string())))
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
    if let Some(mode) = crate::resolved_default_shunt_control_mode(
        case_mode,
        options.default_shunt_control_mode_override.as_deref(),
    ) {
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
            Arc::new(arrow::array::BooleanArray::from(vec![
                modern_grid_profile_value,
            ])),
            Arc::new(ibr_penetration_pct_arr.finish()),
            Arc::new(arrow::array::BooleanArray::from(vec![has_ibr_value])),
            Arc::new(arrow::array::BooleanArray::from(vec![
                has_smart_valve_value,
            ])),
            Arc::new(arrow::array::BooleanArray::from(vec![
                has_multi_terminal_dc_value,
            ])),
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

pub fn build_buses_batch(
    buses: &[Bus],
    agg_by_bus: &HashMap<u32, BusAggregate>,
) -> Result<RecordBatch> {
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
        let agg = agg_by_bus.get(&bus.number).cloned().unwrap_or_default();
        // For generator buses use the EPC voltage schedule (vsched) as the PV regulation
        // target. The PSLF continuation-line VS token is a placeholder (≈1.0) and must NOT
        // override the correct schedule from the bus record.
        let (v_mag, v_ang) = if agg.has_generator {
            sanitize_bus_voltage(bus.vsched, bus.angle)
        } else {
            sanitize_bus_voltage(bus.volt, bus.angle)
        };
        let mut q_min_val = agg.q_min;
        let mut q_max_val = agg.q_max;
        if q_min_val > q_max_val {
            std::mem::swap(&mut q_min_val, &mut q_max_val);
        }

        bus_id.append_value(bus.number as i32);
        name.append_value(bus.name.as_ref());
        // PSLF EPC bus records store ty=1 for all connected buses (PV/PQ is implicit from
        // attached devices). Infer type-2 (PV) from generator presence so that raptrix-core's
        // Q-switch mechanism fires correctly. Type-3 (slack) is auto-assigned by core when no
        // explicit swing bus is present in the EPC (area swing_bus=0 for Texas cases).
        let type_code = if agg.has_generator {
            2i8
        } else {
            canonical_bus_type_code(bus.ty)
        };
        bus_type.append_value(type_code);
        p_sched.append_value(agg.p_sched);
        q_sched.append_value(agg.q_sched);
        v_mag_set.append_value(v_mag);
        v_ang_set.append_value(v_ang);
        q_min.append_value(q_min_val);
        q_max.append_value(q_max_val);
        g_shunt.append_value(agg.g_shunt);
        b_shunt.append_value(agg.b_shunt);
        area.append_value(bus.area as i32);
        zone.append_value(bus.zone as i32);
        if bus.owner > 0 {
            owner.append_value(bus.owner as i32);
        } else {
            owner.append_null();
        }
        v_min.append_value(0.9);
        v_max.append_value(1.1);
        p_min_agg.append_value(agg.p_min_agg);
        p_max_agg.append_value(agg.p_max_agg);
        nominal_kv.append_value(bus.kv);
        bus_uuid.append_value(format!("pslf:bus:{}", bus.number));
        qd_load_pu.append_value(agg.qd_load_pu);
        qg_sched_pu.append_value(agg.qg_sched_pu);
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

pub fn build_generators_batch(
    generators: &[Generator],
    dyd_generators: &[DydGeneratorData],
    ibr_subtype_by_gen: &HashMap<(u32, String), String>,
    sanitization_stats: &mut GeneratorQSanitizationStats,
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
    let mut mrid = StringBuilder::new();
    let mut params = MapBuilder::new(
        Some(map_field_names),
        StringBuilder::new(),
        Float64Builder::new(),
    );

    for (idx, generator) in generators.iter().enumerate() {
        let key = (generator.bus, generator.id.to_string());
        let subtype = ibr_subtype_by_gen.get(&key).cloned();
        let ibr = subtype.is_some();
        let (q_min_export, q_max_export) =
            sanitize_generator_q_limits(generator.qb, generator.qt, sanitization_stats);

        generator_id.append_value((idx + 1) as i32);
        bus_id.append_value(generator.bus as i32);
        name_b.append_null();
        unit_type.append_value("unit");
        hierarchy_level.append_value("unit");
        parent_generator_id.append_null();
        aggregation_count.append_null();
        status.append_value(generator.status != 0);
        is_ibr.append_value(ibr);
        if let Some(value) = subtype {
            ibr_subtype.append_value(value.as_str());
        } else {
            ibr_subtype.append_null();
        }
        p_sched_mw.append_value(generator.pg);
        q_sched_mvar.append_value(generator.qg);
        p_min_mw.append_value(generator.pb);
        p_max_mw.append_value(generator.pt);
        q_min_mvar.append_value(q_min_export);
        q_max_mvar.append_value(q_max_export);
        mbase_mva.append_value(generator.mbase);
        uol_mw.append_null();
        lol_mw.append_null();
        ramp_rate_up_mw_min.append_null();
        ramp_rate_down_mw_min.append_null();
        owner_id.append_null();
        market_resource_id.append_null();
        controlled_bus_id.append_value(generator_controlled_bus_id(generator));
        mrid.append_value(synth_generator_mrid(generator.bus, generator.id.as_ref()));
        append_pslf_generator_raw_params(&mut params, generator)
            .context("PSLF generator params")?;
        let _ = dyd_generators;
        params
            .append(true)
            .context("building generators.params map entry")?;
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
            Arc::new(mrid.finish()),
        ],
    )
    .context("building generators batch")
}

pub fn build_loads_batch(loads: &[Load], base_mva: f64) -> Result<RecordBatch> {
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

pub fn build_branches_batch(
    branches: &[Branch],
    bus_nominal_kv: &HashMap<u32, f64>,
    base_mva: f64,
) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_BRANCHES).expect("branches schema must exist"));

    let mut branch_id = Int32Builder::new();
    let mut from_bus_id = Int32Builder::new();
    let mut to_bus_id = Int32Builder::new();
    let mut ckt = StringDictionaryBuilder::<Int32Type>::new();
    let mut r = Float64Builder::new();
    let mut x = Float64Builder::new();
    let mut b_shunt = Float64Builder::new();
    let mut tap = Float64Builder::new();
    let mut phase = Float64Builder::new();
    let mut rate_a = Float64Builder::new();
    let mut rate_b = Float64Builder::new();
    let mut rate_c = Float64Builder::new();
    let mut status = BooleanBuilder::new();
    let mut owner_id = Int32Builder::new();
    let mut name_b = StringDictionaryBuilder::<UInt32Type>::new();
    let mut from_nominal_kv = Float64Builder::new();
    let mut to_nominal_kv = Float64Builder::new();
    let mut device_type = StringDictionaryBuilder::<Int32Type>::new();
    let mut control_mode = StringDictionaryBuilder::<Int32Type>::new();
    let mut control_target_flow_mw = Float64Builder::new();
    let mut x_min_pu = Float64Builder::new();
    let mut x_max_pu = Float64Builder::new();
    let mut injected_voltage_mag_pu = Float64Builder::new();
    let mut injected_voltage_angle_deg = Float64Builder::new();
    let mut parent_line_id = Int32Builder::new();
    let mut section_index = Int32Builder::new();
    let mut mrid = StringBuilder::new();
    let map_field_names = MapFieldNames {
        entry: "entries".to_string(),
        key: "key".to_string(),
        value: "value".to_string(),
    };
    let mut facts_params = MapBuilder::new(
        Some(map_field_names),
        StringBuilder::new(),
        Float64Builder::new(),
    );

    for (idx, branch) in branches.iter().enumerate() {
        branch_id.append_value((idx + 1) as i32);
        from_bus_id.append_value(branch.from_bus as i32);
        to_bus_id.append_value(branch.to_bus as i32);
        ckt.append_value(branch.ckt.as_ref());
        let z_base = branch_z_base(branch.from_bus, bus_nominal_kv, base_mva);
        r.append_value(branch.r * z_base);
        x.append_value(branch.x * z_base);
        b_shunt.append_value(branch.b / z_base);
        tap.append_value(1.0);
        phase.append_value(0.0);
        rate_a.append_value(branch.rate_a / base_mva);
        rate_b.append_value(branch.rate_b / base_mva);
        rate_c.append_value(branch.rate_c / base_mva);
        status.append_value(branch.status != 0);
        owner_id.append_null();
        name_b.append_null();
        from_nominal_kv.append_value(resolve_required_branch_nominal_kv(
            branch.from_bus,
            branch.to_bus,
            bus_nominal_kv,
            "branches.from_nominal_kv",
        )?);
        to_nominal_kv.append_value(resolve_required_branch_nominal_kv(
            branch.to_bus,
            branch.from_bus,
            bus_nominal_kv,
            "branches.to_nominal_kv",
        )?);
        device_type.append_null();
        control_mode.append_null();
        control_target_flow_mw.append_null();
        x_min_pu.append_null();
        x_max_pu.append_null();
        injected_voltage_mag_pu.append_null();
        injected_voltage_angle_deg.append_null();
        parent_line_id.append_null();
        section_index.append_null();
        mrid.append_value(synth_branch_mrid(
            branch.from_bus,
            branch.to_bus,
            branch.ckt.as_ref(),
        ));
        facts_params
            .append(false)
            .context("building branches.facts_params null entry")?;
    }

    let facts_params_arr = facts_params.finish();
    let facts_params_target = schema
        .field_with_name("facts_params")
        .expect("facts_params field must exist in branches schema")
        .data_type()
        .clone();
    let facts_params_cast = arrow::compute::cast(&facts_params_arr, &facts_params_target)
        .context("casting branches facts_params")?;

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(branch_id.finish()),
            Arc::new(from_bus_id.finish()),
            Arc::new(to_bus_id.finish()),
            Arc::new(ckt.finish()),
            Arc::new(r.finish()),
            Arc::new(x.finish()),
            Arc::new(b_shunt.finish()),
            Arc::new(tap.finish()),
            Arc::new(phase.finish()),
            Arc::new(rate_a.finish()),
            Arc::new(rate_b.finish()),
            Arc::new(rate_c.finish()),
            Arc::new(status.finish()),
            Arc::new(owner_id.finish()),
            Arc::new(name_b.finish()),
            Arc::new(from_nominal_kv.finish()),
            Arc::new(to_nominal_kv.finish()),
            Arc::new(device_type.finish()),
            Arc::new(control_mode.finish()),
            Arc::new(control_target_flow_mw.finish()),
            Arc::new(x_min_pu.finish()),
            Arc::new(x_max_pu.finish()),
            Arc::new(injected_voltage_mag_pu.finish()),
            Arc::new(injected_voltage_angle_deg.finish()),
            facts_params_cast,
            Arc::new(parent_line_id.finish()),
            Arc::new(section_index.finish()),
            Arc::new(mrid.finish()),
        ],
    )
    .context("building branches batch")
}

pub fn build_transformers_2w_batch(
    transformers: &[Transformer2W],
    bus_nominal_kv: &HashMap<u32, f64>,
    base_mva: f64,
    star_leg_mrid_map: &HashMap<(u32, u32), String>,
) -> Result<RecordBatch> {
    let schema =
        Arc::new(table_schema(TABLE_TRANSFORMERS_2W).expect("transformers_2w schema must exist"));

    let mut from_bus_id = Int32Builder::new();
    let mut to_bus_id = Int32Builder::new();
    let mut ckt = StringDictionaryBuilder::<Int32Type>::new();
    let mut r = Float64Builder::new();
    let mut x = Float64Builder::new();
    let mut winding1_r = Float64Builder::new();
    let mut winding1_x = Float64Builder::new();
    let mut winding2_r = Float64Builder::new();
    let mut winding2_x = Float64Builder::new();
    let mut g = Float64Builder::new();
    let mut b = Float64Builder::new();
    let mut tap_ratio = Float64Builder::new();
    let mut nominal_tap_ratio = Float64Builder::new();
    let mut phase_shift = Float64Builder::new();
    let mut vector_group = StringDictionaryBuilder::<Int32Type>::new();
    let mut rate_a = Float64Builder::new();
    let mut rate_b = Float64Builder::new();
    let mut rate_c = Float64Builder::new();
    let mut status = BooleanBuilder::new();
    let mut name_b = StringDictionaryBuilder::<UInt32Type>::new();
    let mut from_nominal_kv = Float64Builder::new();
    let mut to_nominal_kv = Float64Builder::new();
    let mut mrid = StringBuilder::new();

    for t in transformers {
        from_bus_id.append_value(t.from_bus as i32);
        to_bus_id.append_value(t.to_bus as i32);
        ckt.append_value(t.ckt.as_ref());
        let from_kv = resolve_required_transformer_nominal_kv(
            t.from_kv,
            t.from_bus,
            t.to_bus,
            bus_nominal_kv,
            "transformers_2w.from_nominal_kv",
        )?;
        let to_kv = resolve_required_transformer_nominal_kv(
            t.to_kv,
            t.to_bus,
            t.from_bus,
            bus_nominal_kv,
            "transformers_2w.to_nominal_kv",
        )?;
        let z_base = transformer_z_base(from_kv, to_kv, base_mva);
        r.append_value(t.r * z_base);
        x.append_value(t.x * z_base);
        winding1_r.append_value(0.0);
        winding1_x.append_value(0.0);
        winding2_r.append_value(0.0);
        winding2_x.append_value(0.0);
        g.append_value(0.0);
        b.append_value(t.b / z_base);
        let tap = if t.tap > 0.0 { t.tap } else { 1.0 };
        let nominal_tap = if from_kv > 0.0 && to_kv > 0.0 {
            from_kv / to_kv
        } else {
            1.0
        };
        tap_ratio.append_value(tap);
        nominal_tap_ratio.append_value(nominal_tap);
        phase_shift.append_value(t.phase_shift.to_radians());
        vector_group.append_value("unknown");
        rate_a.append_value(t.rate_a / base_mva);
        rate_b.append_value(t.rate_b / base_mva);
        rate_c.append_value(t.rate_c / base_mva);
        status.append_value(t.status != 0);
        name_b.append_null();
        from_nominal_kv.append_value(from_kv);
        to_nominal_kv.append_value(to_kv);
        mrid.append_value(synth_transformer_2w_mrid_with_star_legs(
            t.from_bus,
            t.to_bus,
            t.ckt.as_ref(),
            star_leg_mrid_map,
        ));
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(from_bus_id.finish()),
            Arc::new(to_bus_id.finish()),
            Arc::new(ckt.finish()),
            Arc::new(r.finish()),
            Arc::new(x.finish()),
            Arc::new(winding1_r.finish()),
            Arc::new(winding1_x.finish()),
            Arc::new(winding2_r.finish()),
            Arc::new(winding2_x.finish()),
            Arc::new(g.finish()),
            Arc::new(b.finish()),
            Arc::new(tap_ratio.finish()),
            Arc::new(nominal_tap_ratio.finish()),
            Arc::new(phase_shift.finish()),
            Arc::new(vector_group.finish()),
            Arc::new(rate_a.finish()),
            Arc::new(rate_b.finish()),
            Arc::new(rate_c.finish()),
            Arc::new(status.finish()),
            Arc::new(name_b.finish()),
            Arc::new(from_nominal_kv.finish()),
            Arc::new(to_nominal_kv.finish()),
            Arc::new(mrid.finish()),
        ],
    )
    .context("building transformers_2w batch")
}

pub fn build_transformers_3w_batch(
    transformers: &[Transformer3W],
    bus_nominal_kv: &HashMap<u32, f64>,
    base_mva: f64,
) -> Result<RecordBatch> {
    let schema =
        Arc::new(table_schema(TABLE_TRANSFORMERS_3W).expect("transformers_3w schema must exist"));

    let mut bus_h_id = Int32Builder::new();
    let mut bus_m_id = Int32Builder::new();
    let mut bus_l_id = Int32Builder::new();
    let mut star_bus_id = Int32Builder::new();
    let mut ckt = StringDictionaryBuilder::<Int32Type>::new();
    let mut r_hm = Float64Builder::new();
    let mut x_hm = Float64Builder::new();
    let mut r_hl = Float64Builder::new();
    let mut x_hl = Float64Builder::new();
    let mut r_ml = Float64Builder::new();
    let mut x_ml = Float64Builder::new();
    let mut tap_h = Float64Builder::new();
    let mut tap_m = Float64Builder::new();
    let mut tap_l = Float64Builder::new();
    let mut phase_shift = Float64Builder::new();
    let mut vector_group = StringDictionaryBuilder::<Int32Type>::new();
    let mut rate_a = Float64Builder::new();
    let mut rate_b = Float64Builder::new();
    let mut rate_c = Float64Builder::new();
    let mut status = BooleanBuilder::new();
    let mut name_b = StringDictionaryBuilder::<UInt32Type>::new();
    let mut nominal_kv_h = Float64Builder::new();
    let mut nominal_kv_m = Float64Builder::new();
    let mut nominal_kv_l = Float64Builder::new();
    let mut mrid = StringBuilder::new();

    for t in transformers {
        bus_h_id.append_value(t.bus_h as i32);
        bus_m_id.append_value(t.bus_m as i32);
        bus_l_id.append_value(t.bus_l as i32);
        star_bus_id.append_value(t.star_bus_id as i32);
        ckt.append_value(t.ckt.as_ref());
        r_hm.append_value(t.r_hm);
        x_hm.append_value(t.x_hm);
        r_hl.append_value(t.r_lh);
        x_hl.append_value(t.x_lh);
        r_ml.append_value(t.r_ml);
        x_ml.append_value(t.x_ml);
        tap_h.append_value(if t.tap_h > 0.0 { t.tap_h } else { 1.0 });
        tap_m.append_value(if t.tap_m > 0.0 { t.tap_m } else { 1.0 });
        tap_l.append_value(if t.tap_l > 0.0 { t.tap_l } else { 1.0 });
        phase_shift.append_value(t.phase_shift_deg.to_radians());
        vector_group.append_value("unknown");
        rate_a.append_value(t.rate_h / base_mva);
        rate_b.append_value(t.rate_m / base_mva);
        rate_c.append_value(t.rate_l / base_mva);
        status.append_value(t.status != 0);
        name_b.append_null();
        let kv_h = resolve_required_transformer_nominal_kv(
            t.nominal_kv_h,
            t.bus_h,
            t.bus_m,
            bus_nominal_kv,
            "transformers_3w.nominal_kv_h",
        )?;
        let kv_m = resolve_required_transformer_nominal_kv(
            t.nominal_kv_m,
            t.bus_m,
            t.bus_h,
            bus_nominal_kv,
            "transformers_3w.nominal_kv_m",
        )?;
        let kv_l = resolve_required_transformer_nominal_kv(
            t.nominal_kv_l,
            t.bus_l,
            t.bus_h,
            bus_nominal_kv,
            "transformers_3w.nominal_kv_l",
        )?;
        nominal_kv_h.append_value(kv_h);
        nominal_kv_m.append_value(kv_m);
        nominal_kv_l.append_value(kv_l);
        mrid.append_value(synth_transformer_3w_mrid(
            t.bus_h,
            t.bus_m,
            t.bus_l,
            t.ckt.as_ref(),
        ));
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(bus_h_id.finish()),
            Arc::new(bus_m_id.finish()),
            Arc::new(bus_l_id.finish()),
            Arc::new(star_bus_id.finish()),
            Arc::new(ckt.finish()),
            Arc::new(r_hm.finish()),
            Arc::new(x_hm.finish()),
            Arc::new(r_hl.finish()),
            Arc::new(x_hl.finish()),
            Arc::new(r_ml.finish()),
            Arc::new(x_ml.finish()),
            Arc::new(tap_h.finish()),
            Arc::new(tap_m.finish()),
            Arc::new(tap_l.finish()),
            Arc::new(phase_shift.finish()),
            Arc::new(vector_group.finish()),
            Arc::new(rate_a.finish()),
            Arc::new(rate_b.finish()),
            Arc::new(rate_c.finish()),
            Arc::new(status.finish()),
            Arc::new(name_b.finish()),
            Arc::new(nominal_kv_h.finish()),
            Arc::new(nominal_kv_m.finish()),
            Arc::new(nominal_kv_l.finish()),
            Arc::new(mrid.finish()),
        ],
    )
    .context("building transformers_3w batch")
}

pub fn build_fixed_shunts_batch(shunts: &[FixedShunt], base_mva: f64) -> Result<RecordBatch> {
    let schema =
        Arc::new(table_schema(TABLE_FIXED_SHUNTS).expect("fixed_shunts schema must exist"));

    let mut bus_id = Int32Builder::new();
    let mut id = StringDictionaryBuilder::<Int32Type>::new();
    let mut status = BooleanBuilder::new();
    let mut g_pu = Float64Builder::new();
    let mut b_pu = Float64Builder::new();

    for shunt in shunts {
        bus_id.append_value(shunt.bus as i32);
        id.append_value(shunt.id.as_ref());
        status.append_value(shunt.status != 0);
        g_pu.append_value(shunt.g / base_mva);
        b_pu.append_value(shunt.b / base_mva);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(bus_id.finish()),
            Arc::new(id.finish()),
            Arc::new(status.finish()),
            Arc::new(g_pu.finish()),
            Arc::new(b_pu.finish()),
        ],
    )
    .context("building fixed_shunts batch")
}

fn estimate_current_step(target_binit: f64, steps: &[f64]) -> i32 {
    if steps.is_empty() {
        return 0;
    }
    let mut best_step = 0usize;
    let mut best_error = target_binit.abs();
    let mut cumulative = 0.0;
    for (idx, step) in steps.iter().enumerate() {
        cumulative += *step;
        let error = (cumulative - target_binit).abs();
        if error < best_error - 1.0e-12
            || ((error - best_error).abs() <= 1.0e-12 && (idx + 1) > best_step)
        {
            best_error = error;
            best_step = idx + 1;
        }
    }
    best_step as i32
}

pub fn build_switched_shunts_batch(
    shunts: &[SwitchedShunt],
    _base_mva: f64,
) -> Result<RecordBatch> {
    let schema =
        Arc::new(table_schema(TABLE_SWITCHED_SHUNTS).expect("switched_shunts schema must exist"));

    let mut bus_id = Int32Builder::new();
    let mut status = BooleanBuilder::new();
    let mut v_low = Float64Builder::new();
    let mut v_high = Float64Builder::new();
    let inner_field = Arc::new(arrow::datatypes::Field::new(
        "item",
        arrow::datatypes::DataType::Float64,
        false,
    ));
    let mut b_steps = ListBuilder::new(Float64Builder::new()).with_field(inner_field);
    let mut current_step = Int32Builder::new();
    let mut b_init_pu = Float64Builder::new();
    let mut shunt_id = StringDictionaryBuilder::<Int32Type>::new();
    let mut bus_shunt_counter: HashMap<u32, u32> = HashMap::new();

    for shunt in shunts {
        bus_id.append_value(shunt.bus as i32);
        status.append_value(shunt.status != 0);
        v_low.append_value(shunt.vswlo);
        v_high.append_value(shunt.vswhi);

        let mut step_values_pu = Vec::with_capacity(shunt.steps.len());
        for &step_pu in &shunt.steps {
            if step_pu > 0.0 {
                step_values_pu.push(step_pu);
            }
        }
        for &step_pu in &step_values_pu {
            b_steps.values().append_value(step_pu);
        }
        b_steps.append(true);

        let binit_pu = shunt.b_init;
        current_step.append_value(estimate_current_step(binit_pu, &step_values_pu));
        b_init_pu.append_value(binit_pu);

        let n = {
            let cnt = bus_shunt_counter.entry(shunt.bus).or_insert(0);
            *cnt += 1;
            *cnt
        };
        shunt_id.append_value(format!("{}_shunt_{}", shunt.bus, n));
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(bus_id.finish()),
            Arc::new(status.finish()),
            Arc::new(v_low.finish()),
            Arc::new(v_high.finish()),
            Arc::new(b_steps.finish()),
            Arc::new(current_step.finish()),
            Arc::new(b_init_pu.finish()),
            Arc::new(shunt_id.finish()),
        ],
    )
    .context("building switched_shunts batch")
}

pub fn build_switched_shunt_banks_batch(rows: &[SwitchedShuntBankRow]) -> Result<RecordBatch> {
    let schema = Arc::new(
        table_schema(TABLE_SWITCHED_SHUNT_BANKS).expect("switched_shunt_banks schema must exist"),
    );

    let mut shunt_id = Int32Builder::new();
    let mut bank_id = Int32Builder::new();
    let mut b_mvar = Float64Builder::new();
    let mut status = BooleanBuilder::new();
    let mut step = Int32Builder::new();

    for row in rows {
        shunt_id.append_value(row.shunt_id);
        bank_id.append_value(row.bank_id);
        b_mvar.append_value(row.b_mvar);
        status.append_value(row.status);
        step.append_value(row.step);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(shunt_id.finish()),
            Arc::new(bank_id.finish()),
            Arc::new(b_mvar.finish()),
            Arc::new(status.finish()),
            Arc::new(step.finish()),
        ],
    )
    .context("building switched_shunt_banks batch")
}

pub fn build_areas_batch(areas: &[Area]) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_AREAS).expect("areas schema must exist"));

    let mut area_id = Int32Builder::new();
    let mut name = StringDictionaryBuilder::<Int32Type>::new();
    let mut interchange_mw = Float64Builder::new();

    for area in areas {
        area_id.append_value(area.number as i32);
        name.append_value(area.name.as_ref());
        interchange_mw.append_value(area.desired_net_interchange);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(area_id.finish()),
            Arc::new(name.finish()),
            Arc::new(interchange_mw.finish()),
        ],
    )
    .context("building areas batch")
}

pub fn build_zones_batch(zones: &[Zone]) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_ZONES).expect("zones schema must exist"));

    let mut zone_id = Int32Builder::new();
    let mut name = StringDictionaryBuilder::<Int32Type>::new();

    for zone in zones {
        zone_id.append_value(zone.number as i32);
        name.append_value(zone.name.as_ref());
    }

    RecordBatch::try_new(
        schema,
        vec![Arc::new(zone_id.finish()), Arc::new(name.finish())],
    )
    .context("building zones batch")
}

pub fn build_owners_batch(owners: &[Owner]) -> Result<RecordBatch> {
    let schema = Arc::new(table_schema(TABLE_OWNERS).expect("owners schema must exist"));

    let mut owner_id = Int32Builder::new();
    let mut name = StringBuilder::new();
    let mut short_name = StringBuilder::new();
    let mut owner_type = StringBuilder::new();
    let map_field_names = MapFieldNames {
        entry: "entries".to_string(),
        key: "key".to_string(),
        value: "value".to_string(),
    };
    let mut params = MapBuilder::new(
        Some(map_field_names),
        StringBuilder::new(),
        Float64Builder::new(),
    );

    for owner in owners {
        owner_id.append_value(owner.number as i32);
        name.append_value(owner.name.as_ref());
        short_name.append_null();
        owner_type.append_null();
        params
            .append(false)
            .context("building owners.params null entry")?;
    }

    let params_arr = params.finish();
    let params_target_type = schema
        .field_with_name("params")
        .expect("params field must exist in owners schema")
        .data_type()
        .clone();
    let params_cast =
        arrow::compute::cast(&params_arr, &params_target_type).context("casting owners params")?;

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(owner_id.finish()),
            Arc::new(name.finish()),
            Arc::new(short_name.finish()),
            Arc::new(owner_type.finish()),
            params_cast,
        ],
    )
    .context("building owners batch")
}

pub fn build_dynamics_models_batch(records: &[DydModelData]) -> Result<RecordBatch> {
    let schema =
        Arc::new(table_schema(TABLE_DYNAMICS_MODELS).expect("dynamics_models schema must exist"));

    let mut bus_id = Int32Builder::new();
    let mut gen_id = StringDictionaryBuilder::<Int32Type>::new();
    let mut model_type = StringDictionaryBuilder::<Int32Type>::new();
    let map_field_names = MapFieldNames {
        entry: "entries".to_string(),
        key: "key".to_string(),
        value: "value".to_string(),
    };
    let mut params = MapBuilder::new(
        Some(map_field_names),
        StringBuilder::new(),
        Float64Builder::new(),
    );

    for rec in records {
        bus_id.append_value(rec.bus as i32);
        gen_id.append_value(rec.id.as_ref());
        model_type.append_value(rec.model_type.as_ref());

        for (idx, value) in rec.params.iter().enumerate() {
            params.keys().append_value(format!("p{idx}"));
            params.values().append_value(*value);
        }
        params
            .append(true)
            .context("building dynamics params map entry")?;
    }

    let params_arr = params.finish();
    let params_target_type = schema
        .field_with_name("params")
        .expect("params field must exist in dynamics_models schema")
        .data_type()
        .clone();
    let params_cast = arrow::compute::cast(&params_arr, &params_target_type)
        .context("casting dynamics params map")?;

    let perc1_params_type = schema
        .field_with_name("perc1_params")
        .expect("dynamics_models schema must include perc1_params (v0.10.0+)")
        .data_type()
        .clone();
    let perc1_params_col = new_null_array(&perc1_params_type, records.len());

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(bus_id.finish()),
            Arc::new(gen_id.finish()),
            Arc::new(model_type.finish()),
            params_cast,
            perc1_params_col,
        ],
    )
    .context("building dynamics_models batch")
}

pub fn consolidate_switched_shunts_by_bus(mut shunts: Vec<SwitchedShunt>) -> Vec<SwitchedShunt> {
    let mut order: Vec<u32> = Vec::new();
    let mut by_bus: HashMap<u32, SwitchedShunt> = HashMap::new();

    for shunt in shunts.drain(..) {
        let bus = shunt.bus;
        if let Some(existing) = by_bus.get_mut(&bus) {
            existing.b_init += shunt.b_init;
            existing.bank_pairs.extend(shunt.bank_pairs);
            existing.steps.extend(shunt.steps);
            existing.vswlo = existing.vswlo.min(shunt.vswlo);
            existing.vswhi = existing.vswhi.max(shunt.vswhi);
            if shunt.status == 0 {
                existing.status = 0;
            }
        } else {
            order.push(bus);
            by_bus.insert(bus, shunt);
        }
    }

    order
        .into_iter()
        .filter_map(|bus| by_bus.remove(&bus))
        .collect()
}

pub fn prepare_network_for_export(network: &mut Network) {
    derive_switched_shunt_banks(network);
}

pub fn build_bus_aggregates_for_export(network: &Network) -> HashMap<u32, BusAggregate> {
    build_bus_aggregates(network)
}

const SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE: u32 = mrid::SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE;

pub fn validate_export_invariants(
    table_batches: &HashMap<&'static str, RecordBatch>,
    transformer_mode: TransformerRepresentationMode,
) -> Result<()> {
    let branches = table_batches
        .get(TABLE_BRANCHES)
        .ok_or_else(|| anyhow::anyhow!("missing branches batch"))?;
    validate_nonnegative_finite_column(branches, "rate_a", TABLE_BRANCHES)?;
    validate_nonnegative_finite_column(branches, "rate_b", TABLE_BRANCHES)?;
    validate_nonnegative_finite_column(branches, "rate_c", TABLE_BRANCHES)?;

    let transformers = table_batches
        .get(TABLE_TRANSFORMERS_2W)
        .ok_or_else(|| anyhow::anyhow!("missing transformers_2w batch"))?;
    validate_nonnegative_finite_column(transformers, "rate_a", TABLE_TRANSFORMERS_2W)?;
    validate_nonnegative_finite_column(transformers, "rate_b", TABLE_TRANSFORMERS_2W)?;
    validate_nonnegative_finite_column(transformers, "rate_c", TABLE_TRANSFORMERS_2W)?;

    let status = transformers
        .column_by_name("status")
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.status missing"))?
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.status is not Boolean"))?;
    let tap_ratio = transformers
        .column_by_name("tap_ratio")
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.tap_ratio missing"))?
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.tap_ratio is not Float64"))?;
    let nominal_tap_ratio = transformers
        .column_by_name("nominal_tap_ratio")
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.nominal_tap_ratio missing"))?
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.nominal_tap_ratio is not Float64"))?;

    for i in 0..transformers.num_rows() {
        if !status.value(i) {
            continue;
        }
        let tap = tap_ratio.value(i);
        if !tap.is_finite() || tap <= 0.0 {
            bail!(
                "export invariant violation: in-service transformers with invalid tap_ratio at 1-based row {}",
                i + 1
            );
        }
        let nominal = nominal_tap_ratio.value(i);
        if !nominal.is_finite() || nominal <= 0.0 {
            bail!(
                "export invariant violation: in-service transformers with invalid nominal_tap_ratio at 1-based row {}",
                i + 1
            );
        }
    }

    let transformers_3w = table_batches
        .get(TABLE_TRANSFORMERS_3W)
        .ok_or_else(|| anyhow::anyhow!("missing transformers_3w batch"))?;
    validate_nonnegative_finite_column(transformers_3w, "rate_a", TABLE_TRANSFORMERS_3W)?;
    validate_nonnegative_finite_column(transformers_3w, "rate_b", TABLE_TRANSFORMERS_3W)?;
    validate_nonnegative_finite_column(transformers_3w, "rate_c", TABLE_TRANSFORMERS_3W)?;

    validate_transformer_representation_mode(transformers, transformers_3w, transformer_mode)?;

    Ok(())
}

fn validate_nonnegative_finite_column(
    batch: &RecordBatch,
    column_name: &str,
    table_name: &str,
) -> Result<()> {
    let values = batch
        .column_by_name(column_name)
        .ok_or_else(|| anyhow::anyhow!("{}.{} missing", table_name, column_name))?
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| anyhow::anyhow!("{}.{} is not Float64", table_name, column_name))?;

    for i in 0..batch.num_rows() {
        let v = values.value(i);
        if !v.is_finite() || v < 0.0 {
            bail!(
                "export invariant violation: {}.{} has negative or non-finite value at 1-based row {}",
                table_name,
                column_name,
                i + 1
            );
        }
    }
    Ok(())
}

fn validate_transformer_representation_mode(
    transformers_2w: &RecordBatch,
    transformers_3w: &RecordBatch,
    transformer_mode: TransformerRepresentationMode,
) -> Result<()> {
    let from_2w = transformers_2w
        .column_by_name("from_bus_id")
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.from_bus_id missing"))?
        .as_any()
        .downcast_ref::<Int32Array>()
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.from_bus_id is not Int32"))?;
    let to_2w = transformers_2w
        .column_by_name("to_bus_id")
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.to_bus_id missing"))?
        .as_any()
        .downcast_ref::<Int32Array>()
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.to_bus_id is not Int32"))?;
    let status_2w = transformers_2w
        .column_by_name("status")
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.status missing"))?
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or_else(|| anyhow::anyhow!("transformers_2w.status is not Boolean"))?;

    let status_3w = transformers_3w
        .column_by_name("status")
        .ok_or_else(|| anyhow::anyhow!("transformers_3w.status missing"))?
        .as_any()
        .downcast_ref::<BooleanArray>()
        .ok_or_else(|| anyhow::anyhow!("transformers_3w.status is not Boolean"))?;

    let mut active_star_buses = 0usize;
    for row in 0..transformers_2w.num_rows() {
        if !status_2w.value(row) {
            continue;
        }
        let from = from_2w.value(row);
        let to = to_2w.value(row);
        if from > SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE as i32
            || to > SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE as i32
        {
            active_star_buses += 1;
        }
    }

    let active_3w_count = (0..transformers_3w.num_rows())
        .filter(|&row| status_3w.value(row))
        .count();

    match transformer_mode {
        TransformerRepresentationMode::Expanded => {
            if active_3w_count > 0 {
                bail!(
                    "export invariant violation: transformer mode 'expanded' requires zero active transformers_3w rows, found {active_3w_count}"
                );
            }
        }
        TransformerRepresentationMode::Native3W => {
            if active_star_buses > 0 {
                bail!(
                    "export invariant violation: transformer mode 'native_3w' forbids active star-leg transformers_2w rows, found {active_star_buses}"
                );
            }
        }
    }

    Ok(())
}
