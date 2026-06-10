// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// Compare two .rpf files table-by-table (row counts + selected numeric columns).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use arrow::array::{Array, Float64Array, Int32Array};
use clap::Parser;
use raptrix_cim_arrow::{
    TABLE_BRANCHES, TABLE_BUSES, TABLE_GENERATORS, TABLE_LOADS, TABLE_TRANSFORMERS_2W,
    TABLE_TRANSFORMERS_3W, read_rpf_tables, summarize_rpf,
};

#[derive(Parser)]
#[command(about = "Compare two RPF files (row counts + optional bus spot-checks)")]
struct Args {
    /// Left-hand (e.g. PSLF-derived) RPF path
    left: PathBuf,
    /// Right-hand (e.g. PSSE-derived) RPF path
    right: PathBuf,
    /// Bus ID for buses/generators/loads spot-check (default: 110001 loads/buses, 111180 generators)
    #[arg(long)]
    bus_id: Option<i32>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    compare(&args.left, &args.right, args.bus_id)?;
    Ok(())
}

fn compare(left: &Path, right: &Path, bus_id: Option<i32>) -> Result<()> {
    let left_sum = summarize_rpf(left).with_context(|| format!("summarize {}", left.display()))?;
    let right_sum =
        summarize_rpf(right).with_context(|| format!("summarize {}", right.display()))?;

    println!(
        "Left:  {} ({} tables, {} rows)",
        left.display(),
        left_sum.tables.len(),
        left_sum.total_rows
    );
    println!(
        "Right: {} ({} tables, {} rows)",
        right.display(),
        right_sum.tables.len(),
        right_sum.total_rows
    );
    println!();

    let left_map: BTreeMap<_, _> = left_sum
        .tables
        .iter()
        .map(|t| (t.table_name.as_str(), t.rows))
        .collect();
    let right_map: BTreeMap<_, _> = right_sum
        .tables
        .iter()
        .map(|t| (t.table_name.as_str(), t.rows))
        .collect();

    let all_tables: BTreeMap<_, _> = left_map
        .keys()
        .chain(right_map.keys())
        .map(|k| (*k, ()))
        .collect();

    let mut mismatches = 0usize;
    println!(
        "{:<30} {:>10} {:>10} {:>8}",
        "table", "left", "right", "match"
    );
    println!("{}", "-".repeat(62));
    for table in all_tables.keys() {
        let l = left_map.get(table).copied().unwrap_or(0);
        let r = right_map.get(table).copied().unwrap_or(0);
        let ok = l == r;
        if !ok {
            mismatches += 1;
        }
        println!(
            "{:<30} {:>10} {:>10} {:>8}",
            table,
            l,
            r,
            if ok { "OK" } else { "DIFF" }
        );
    }

    println!();
    compare_key_fields(left, right, bus_id)?;

    if mismatches > 0 {
        bail!("{mismatches} table(s) have row-count mismatches");
    }
    Ok(())
}

fn compare_key_fields(left: &Path, right: &Path, bus_id: Option<i32>) -> Result<()> {
    let left_tables = read_rpf_tables(left)?;
    let right_tables = read_rpf_tables(right)?;
    let left_by_name: BTreeMap<_, _> = left_tables.into_iter().collect();
    let right_by_name: BTreeMap<_, _> = right_tables.into_iter().collect();

    let bus_spot = bus_id.unwrap_or(110001);
    let gen_spot = bus_id.unwrap_or(111180);

    for table in [
        TABLE_BUSES,
        TABLE_GENERATORS,
        TABLE_LOADS,
        TABLE_BRANCHES,
        TABLE_TRANSFORMERS_2W,
        TABLE_TRANSFORMERS_3W,
    ] {
        let Some(lb) = left_by_name.get(table) else {
            continue;
        };
        let Some(rb) = right_by_name.get(table) else {
            continue;
        };
        if lb.num_rows() == 0 && rb.num_rows() == 0 {
            continue;
        }
        println!("--- {table} field spot-check ---");
        match table {
            TABLE_BUSES => spot_check_bus(lb, rb, bus_spot)?,
            TABLE_GENERATORS => spot_check_generators(lb, rb, gen_spot)?,
            TABLE_LOADS => spot_check_loads(lb, rb, bus_spot)?,
            _ => {
                println!("  (row counts only for {table})");
            }
        }
    }
    Ok(())
}

fn col_i32(batch: &arrow::record_batch::RecordBatch, name: &str) -> Result<Int32Array> {
    let idx = batch.schema().index_of(name)?;
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Int32Array>()
        .with_context(|| format!("column {name} is not Int32"))?;
    Ok(arr.clone())
}

fn col_f64(batch: &arrow::record_batch::RecordBatch, name: &str) -> Result<Float64Array> {
    let idx = batch.schema().index_of(name)?;
    let arr = batch
        .column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .with_context(|| format!("column {name} is not Float64"))?;
    Ok(arr.clone())
}

fn spot_check_bus(
    l: &arrow::record_batch::RecordBatch,
    r: &arrow::record_batch::RecordBatch,
    target: i32,
) -> Result<()> {
    let l_id = col_i32(l, "bus_id")?;
    let r_id = col_i32(r, "bus_id")?;
    let l_v = col_f64(l, "v_mag_set")?;
    let r_v = col_f64(r, "v_mag_set")?;
    let l_a = col_f64(l, "v_ang_set")?;
    let r_a = col_f64(r, "v_ang_set")?;
    let l_qmin = col_f64(l, "q_min")?;
    let r_qmin = col_f64(r, "q_min")?;
    let l_qmax = col_f64(l, "q_max")?;
    let r_qmax = col_f64(r, "q_max")?;

    let li = (0..l_id.len()).find(|&i| l_id.value(i) == target);
    let ri = (0..r_id.len()).find(|&i| r_id.value(i) == target);
    match (li, ri) {
        (Some(li), Some(ri)) => {
            println!(
                "  bus {target}: v_mag left={:.6} right={:.6}  v_ang left={:.6} rad right={:.6} rad",
                l_v.value(li),
                r_v.value(ri),
                l_a.value(li),
                r_a.value(ri)
            );
            println!(
                "           q_min left={:.6} right={:.6}  q_max left={:.6} right={:.6}",
                l_qmin.value(li),
                r_qmin.value(ri),
                l_qmax.value(li),
                r_qmax.value(ri)
            );
        }
        _ => println!("  bus {target} not found in one or both files"),
    }
    Ok(())
}

fn spot_check_generators(
    l: &arrow::record_batch::RecordBatch,
    r: &arrow::record_batch::RecordBatch,
    target: i32,
) -> Result<()> {
    let l_bus = col_i32(l, "bus_id")?;
    let r_bus = col_i32(r, "bus_id")?;
    let l_p = col_f64(l, "p_sched_mw")?;
    let r_p = col_f64(r, "p_sched_mw")?;
    let l_qmin = col_f64(l, "q_min_mvar")?;
    let r_qmin = col_f64(r, "q_min_mvar")?;
    let l_qmax = col_f64(l, "q_max_mvar")?;
    let r_qmax = col_f64(r, "q_max_mvar")?;

    let li = (0..l_bus.len()).find(|&i| l_bus.value(i) == target);
    let ri = (0..r_bus.len()).find(|&i| r_bus.value(i) == target);
    match (li, ri) {
        (Some(li), Some(ri)) => {
            println!(
                "  gen bus {target}: p_sched left={:.3} MW right={:.3} MW  q_min left={:.3} right={:.3}  q_max left={:.3} right={:.3}",
                l_p.value(li),
                r_p.value(ri),
                l_qmin.value(li),
                r_qmin.value(ri),
                l_qmax.value(li),
                r_qmax.value(ri)
            );
        }
        _ => println!("  gen bus {target} not found in one or both files"),
    }
    Ok(())
}

fn spot_check_loads(
    l: &arrow::record_batch::RecordBatch,
    r: &arrow::record_batch::RecordBatch,
    target: i32,
) -> Result<()> {
    let l_bus = col_i32(l, "bus_id")?;
    let r_bus = col_i32(r, "bus_id")?;
    let l_p = col_f64(l, "p_pu")?;
    let r_p = col_f64(r, "p_pu")?;

    let li = (0..l_bus.len()).find(|&i| l_bus.value(i) == target);
    let ri = (0..r_bus.len()).find(|&i| r_bus.value(i) == target);
    match (li, ri) {
        (Some(li), Some(ri)) => {
            println!(
                "  load bus {target}: p_pu left={:.6} right={:.6}",
                l_p.value(li),
                r_p.value(ri)
            );
        }
        _ => println!("  load bus {target} not found in one or both files"),
    }
    Ok(())
}
