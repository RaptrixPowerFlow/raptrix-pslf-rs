// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! GE PSLF .epc (power flow) and .dyd (dynamics) parser.
//! Pragmatic line-based section parser with quote-aware tokenization.
//! Designed to be robust against the real-world .EPC/.dyd files in tests/networks/.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::models::{
    Area, Bus, DydGeneratorData, DydModelData, FixedShunt, Generator, Load, Network, Owner, SwitchedShunt,
    Transformer2W, Zone,
};

/// Parse a GE PSLF .epc file.
pub fn parse_epc(path: &Path) -> Result<Network> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open EPC file: {}", path.display()))?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader
        .lines()
        .map(|l| l.unwrap_or_default())
        .collect();

    let mut network = Network {
        title: "".into(),
        sbase: 100.0,
        ..Default::default()
    };

    let mut _current_section = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        if line.is_empty() || line.starts_with('!') {
            i += 1;
            continue;
        }

        // Detect section headers
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("title") {
            _current_section = "title".to_string();
            // Next non-empty non-! line is often the title
            i += 1;
            while i < lines.len() {
                let t = lines[i].trim();
                if !t.is_empty() && !t.starts_with('!') && !is_known_section_header(t) {
                    network.title = t.into();
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        if lower.starts_with("solution parameters") {
            _current_section = "solution parameters".to_string();
            i = parse_solution_parameters(&lines, i + 1, &mut network)?;
            continue;
        }

        if lower.starts_with("substation data") {
            _current_section = "substation".to_string();
            i = skip_section(&lines, i + 1)?;
            continue;
        }

        if lower.starts_with("bus data") {
            _current_section = "bus".to_string();
            i = parse_bus_data(&lines, i + 1, &mut network.buses)?;
            continue;
        }

        if lower.starts_with("load data") {
            _current_section = "load".to_string();
            i = parse_load_data(&lines, i + 1, &mut network.loads)?;
            continue;
        }

        if lower.starts_with("generator data") {
            _current_section = "generator".to_string();
            i = parse_generator_data(&lines, i + 1, &mut network.generators)?;
            continue;
        }

        if lower.starts_with("branch data") {
            _current_section = "branch".to_string();
            i = parse_branch_data(&lines, i + 1, &mut network.branches)?;
            continue;
        }

        if lower.starts_with("transformer data") {
            _current_section = "transformer".to_string();
            i = parse_transformer_data(
                &lines,
                i + 1,
                &mut network.transformers,
                &mut network.transformers_3w,
            )?;
            continue;
        }

        if lower.starts_with("shunt data") {
            _current_section = "shunt".to_string();
            i = parse_shunt_data(&lines, i + 1, &mut network.fixed_shunts)?;
            continue;
        }

        if lower.starts_with("svd data") {
            _current_section = "svd".to_string();
            i = parse_svd_data(&lines, i + 1, &mut network.switched_shunts)?;
            continue;
        }

        if lower.starts_with("area data") {
            _current_section = "area".to_string();
            i = parse_area_data(&lines, i + 1, &mut network.areas)?;
            continue;
        }

        if lower.starts_with("zone data") {
            _current_section = "zone".to_string();
            i = parse_zone_data(&lines, i + 1, &mut network.zones)?;
            continue;
        }

        if lower.starts_with("owner data") {
            _current_section = "owner".to_string();
            i = parse_owner_data(&lines, i + 1, &mut network.owners)?;
            continue;
        }

        // Skip other sections (dc, interface, etc.)
        if is_known_section_header(line) {
            _current_section = lower
                .split_whitespace()
                .next()
                .unwrap_or("unknown")
                .to_string();
        }

        i += 1;
    }

    Ok(network)
}

fn is_known_section_header(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.starts_with("title")
        || l.starts_with("comments")
        || l.starts_with("solution parameters")
        || l.starts_with("substation data")
        || l.starts_with("bus data")
        || l.starts_with("load data")
        || l.starts_with("generator data")
        || l.starts_with("branch data")
        || l.starts_with("transformer data")
        || l.starts_with("area data")
        || l.starts_with("zone data")
        || l.starts_with("owner data")
        || l.starts_with("shunt data")
        || l.starts_with("svd data")
        || l.starts_with("switched shunt")
        || l.starts_with("interface data")
        || l.starts_with("interface branch data")
        || l.starts_with("dc bus data")
        || l.starts_with("dc converter data")
        || l.starts_with("z table data")
        || l.starts_with("gcd data")
        || l.starts_with("transaction data")
        || l.starts_with("qtable data")
        || l.starts_with("ba data")
        || l.starts_with("injgroup data")
        || l.starts_with("injgrpelem data")
        || l.starts_with("dc ")
        || l.starts_with("end")
}

fn skip_section(lines: &[String], mut start: usize) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_solution_parameters(
    lines: &[String],
    mut start: usize,
    net: &mut Network,
) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if line.to_ascii_lowercase().starts_with("sbase") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(v) = parts[1].parse::<f64>() {
                    net.sbase = v;
                }
            }
        }
        start += 1;
    }
    Ok(start)
}

/// Returns true when a data line ends with a PSLF record terminator `/`.
fn line_has_continuation(line: &str) -> bool {
    line.trim_end().ends_with('/')
}

fn parse_bus_data(lines: &[String], mut start: usize, buses: &mut Vec<Bus>) -> Result<usize> {
    let mut skip_next = false;
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if skip_next {
            skip_next = false;
            start += 1;
            continue;
        }
        if let Some(bus) = parse_one_bus_line(line) {
            buses.push(bus);
        }
        if line_has_continuation(line) {
            skip_next = true;
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_bus_line(line: &str) -> Option<Bus> {
    // Typical line after header:
    //   110001 "EDNA 1 1    " 138.0000 " "  0  :  1 1.037093  1.037093  -4.242394    7    1 ...
    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 4 {
        return None;
    }

    let number: u32 = tokens[0].parse().ok()?;
    let name = tokens.get(1).map(String::as_str).unwrap_or("").into();
    let colon_pos = tokens.iter().position(|t| t == ":")?;

    let bus = Bus {
        number,
        name,
        kv: tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        ty: tokens
            .get(colon_pos + 1)
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(1),
        vsched: tokens
            .get(colon_pos + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0),
        volt: tokens
            .get(colon_pos + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0),
        angle: tokens
            .get(colon_pos + 4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        area: tokens
            .get(colon_pos + 5)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        zone: tokens
            .get(colon_pos + 6)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        ..Default::default()
    };

    Some(bus)
}

fn parse_load_data(lines: &[String], mut start: usize, loads: &mut Vec<Load>) -> Result<usize> {
    let mut skip_next = false;
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if skip_next {
            skip_next = false;
            start += 1;
            continue;
        }
        if let Some(load) = parse_one_load_line(line) {
            loads.push(load);
        }
        if line_has_continuation(line) {
            skip_next = true;
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_load_line(line: &str) -> Option<Load> {
    let tokens = tokenize_pslf_line(line);
    let colon_pos = tokens.iter().position(|t| t == ":")?;

    Some(Load {
        bus: tokens[0].parse().ok()?,
        id: tokens.get(3).map(String::as_str).unwrap_or("1").into(),
        p: tokens
            .get(colon_pos + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        q: tokens
            .get(colon_pos + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        status: tokens
            .get(colon_pos + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1),
    })
}

fn parse_generator_data(
    lines: &[String],
    mut start: usize,
    gens: &mut Vec<Generator>,
) -> Result<usize> {
    let mut skip_next = false;
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if skip_next {
            skip_next = false;
            start += 1;
            continue;
        }
        if let Some(generator) = parse_one_generator_line(line) {
            gens.push(generator);
        }
        if line_has_continuation(line) {
            skip_next = true;
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_generator_line(line: &str) -> Option<Generator> {
    let tokens = tokenize_pslf_line(line);
    let colon_pos = tokens.iter().position(|t| t == ":")?;

    Some(Generator {
        bus: tokens[0].parse().ok()?,
        id: tokens.get(3).map(String::as_str).unwrap_or("1").into(),
        pg: tokens
            .get(colon_pos + 9)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        pt: tokens
            .get(colon_pos + 10)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        pb: tokens
            .get(colon_pos + 11)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        qg: tokens
            .get(colon_pos + 12)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        mbase: tokens
            .get(colon_pos + 15)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        status: tokens
            .get(colon_pos + 1)
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(1),
        ..Default::default()
    })
}

fn parse_branch_data(
    lines: &[String],
    mut start: usize,
    branches: &mut Vec<crate::models::Branch>,
) -> Result<usize> {
    let mut skip_next = false;
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if skip_next {
            skip_next = false;
            start += 1;
            continue;
        }
        if let Some(branch) = parse_one_branch_line(line) {
            branches.push(branch);
        }
        if line_has_continuation(line) {
            skip_next = true;
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_branch_line(line: &str) -> Option<crate::models::Branch> {
    let tokens = tokenize_pslf_line(line);
    let colon_pos = tokens.iter().position(|t| t == ":")?;
    let from_bus: u32 = tokens[0].parse().ok()?;
    let to_bus: u32 = tokens.get(3).and_then(|s| s.parse().ok())?;

    Some(crate::models::Branch {
        from_bus,
        to_bus,
        ckt: tokens.get(6).map(String::as_str).unwrap_or("1").into(),
        r: tokens
            .get(colon_pos + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        x: tokens
            .get(colon_pos + 3)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        b: tokens
            .get(colon_pos + 4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        rate_a: tokens
            .get(colon_pos + 5)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        rate_b: tokens
            .get(colon_pos + 6)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        rate_c: tokens
            .get(colon_pos + 7)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        status: tokens
            .get(colon_pos + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1),
        from_name: tokens.get(1).map(String::as_str).unwrap_or("").into(),
        to_name: tokens.get(4).map(String::as_str).unwrap_or("").into(),
        ..Default::default()
    })
}

fn parse_transformer_data(
    lines: &[String],
    mut start: usize,
    t2w: &mut Vec<Transformer2W>,
    t3w: &mut Vec<crate::models::Transformer3W>,
) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(mut xfmr) = parse_transformer_header(line) {
            for k in 1..=3 {
                if start + k >= lines.len() {
                    break;
                }
                let cont = lines[start + k].trim();
                if cont.is_empty() || is_known_section_header(cont) {
                    break;
                }
                apply_transformer_continuation(&mut xfmr, k, cont);
            }
            t2w.push(xfmr);
            start += 4;
            continue;
        }
        if let Some((_, t3)) = parse_one_transformer_line(line) {
            if let Some(xfmr) = t3 {
                t3w.push(xfmr);
            }
        }
        start += 1;
    }
    Ok(start)
}

fn parse_transformer_header(line: &str) -> Option<Transformer2W> {
    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 8 {
        return None;
    }
    let from_bus: u32 = tokens[0].parse().ok()?;
    let to_bus: u32 = tokens.get(3).and_then(|s| s.parse().ok())?;
    let ckt = tokens.get(6).map(String::as_str).unwrap_or("1").into();
    let colon_pos = tokens.iter().position(|t| t == ":")?;
    let status = tokens
        .get(colon_pos + 1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let r = tokens
        .get(colon_pos + 5)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let x = tokens
        .get(colon_pos + 6)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    Some(Transformer2W {
        from_bus,
        to_bus,
        ckt,
        r,
        x,
        status,
        from_kv: tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        to_kv: tokens.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        ..Default::default()
    })
}

fn apply_transformer_continuation(xfmr: &mut Transformer2W, line_idx: usize, line: &str) {
    if line_idx != 1 {
        return;
    }
    let nums: Vec<f64> = line
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() >= 2 {
        if nums[0] > 0.0 {
            xfmr.from_kv = nums[0];
        }
        xfmr.tap = nums[1];
    }
    if nums.len() >= 9 {
        xfmr.rate_a = nums[6];
        xfmr.rate_b = nums[7];
        xfmr.rate_c = nums[8];
    }
    if nums.len() >= 16 {
        xfmr.phase_shift = nums[15];
    }
}

fn parse_shunt_data(
    lines: &[String],
    mut start: usize,
    shunts: &mut Vec<FixedShunt>,
) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(shunt) = parse_one_shunt_line(line) {
            shunts.push(shunt);
        }
        if line_has_continuation(line) {
            start += 1;
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_shunt_line(line: &str) -> Option<FixedShunt> {
    let tokens = tokenize_pslf_line(line);
    let colon_pos = tokens.iter().position(|t| t == ":")?;
    let bus: u32 = tokens[0].parse().ok()?;
    Some(FixedShunt {
        bus,
        id: tokens.get(6).map(String::as_str).unwrap_or("1").into(),
        status: tokens
            .get(colon_pos + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1),
        g: tokens
            .get(colon_pos + 9)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        b: tokens
            .get(colon_pos + 10)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
    })
}

fn parse_svd_data(
    lines: &[String],
    mut start: usize,
    shunts: &mut Vec<SwitchedShunt>,
) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(mut shunt) = parse_one_svd_header(line) {
            if line_has_continuation(line) && start + 1 < lines.len() {
                parse_svd_continuation(lines[start + 1].trim(), &mut shunt);
                start += 1;
            }
            shunts.push(shunt);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_svd_header(line: &str) -> Option<SwitchedShunt> {
    let tokens = tokenize_pslf_line(line);
    let colon_pos = tokens.iter().position(|t| t == ":")?;
    let bus: u32 = tokens[0].parse().ok()?;
    let status = tokens
        .get(colon_pos + 1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let modsw = tokens
        .get(colon_pos + 2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let b_init = tokens
        .get(colon_pos + 10)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let vband = tokens
        .get(colon_pos + 13)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.015);
    let vswlo = (1.0_f64 - vband).max(0.0);
    let vswhi = 1.0 + vband;

    Some(SwitchedShunt {
        bus,
        id: tokens.get(6).map(String::as_str).unwrap_or("1").into(),
        modsw,
        status,
        vswlo,
        vswhi,
        b_init,
        ..Default::default()
    })
}

fn parse_svd_continuation(line: &str, shunt: &mut SwitchedShunt) {
    let nums: Vec<f64> = line
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() >= 2 {
        let n_steps = nums[0].round().max(0.0) as u32;
        let b_mvar = nums[1];
        if n_steps > 0 && b_mvar.abs() > 1.0e-12 {
            shunt.bank_pairs.push((n_steps, b_mvar));
            for _ in 0..n_steps {
                shunt.steps.push(b_mvar);
            }
        }
    }
}

fn parse_area_data(lines: &[String], mut start: usize, areas: &mut Vec<Area>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(area) = parse_one_area_line(line) {
            areas.push(area);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_area_line(line: &str) -> Option<Area> {
    let tokens = tokenize_pslf_line(line);
    let number: u32 = tokens.first()?.parse().ok()?;
    let name = tokens.get(1).map(String::as_str).unwrap_or("").into();
    let swing_bus = tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let desired = tokens.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    Some(Area {
        number,
        name,
        swing_bus,
        desired_net_interchange: desired,
    })
}

fn parse_zone_data(lines: &[String], mut start: usize, zones: &mut Vec<Zone>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(zone) = parse_one_zone_line(line) {
            zones.push(zone);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_zone_line(line: &str) -> Option<Zone> {
    let tokens = tokenize_pslf_line(line);
    let number: u32 = tokens.first()?.parse().ok()?;
    let name = tokens.get(1).map(String::as_str).unwrap_or("").into();
    Some(Zone { number, name })
}

fn parse_owner_data(lines: &[String], mut start: usize, owners: &mut Vec<Owner>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(owner) = parse_one_owner_line(line) {
            owners.push(owner);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_owner_line(line: &str) -> Option<Owner> {
    let tokens = tokenize_pslf_line(line);
    let number: u32 = tokens.first()?.parse().ok()?;
    let name = tokens.get(1).map(String::as_str).unwrap_or("").into();
    Some(Owner { number, name })
}

fn parse_one_transformer_line(
    line: &str,
) -> Option<(Option<Transformer2W>, Option<crate::models::Transformer3W>)> {
    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 8 {
        return None;
    }
    let from_bus: u32 = tokens[0].parse().ok()?;
    let to_bus: u32 = tokens.get(3).and_then(|s| s.parse().ok())?;
    let ckt = tokens.get(6).map(String::as_str).unwrap_or("1").into();
    let colon_pos = tokens.iter().position(|t| t == ":")?;
    let status = tokens
        .get(colon_pos + 1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    // PSLF 2-winding primary record: from_bus, to_bus on first line.
    // 3-winding detection deferred — emit as 2W for v1 unless tertiary bus appears later.
    let r = tokens
        .get(colon_pos + 5)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let x = tokens
        .get(colon_pos + 6)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    Some((
        Some(Transformer2W {
            from_bus,
            to_bus,
            ckt,
            r,
            x,
            status,
            from_kv: tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
            to_kv: tokens.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0),
            ..Default::default()
        }),
        None,
    ))
}

/// Very lightweight quote-aware tokenizer for PSLF lines.
/// Handles "name with spaces" and colon separators seen in the real files.
fn tokenize_pslf_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = line.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ':' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.trim().to_string());
                    current.clear();
                }
                tokens.push(":".to_string());
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
        i += 1;
    }
    if !current.is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

// ---------------------------------------------------------------------------
// DYD parser (dynamics)
// ---------------------------------------------------------------------------

pub fn parse_dyd(path: &Path, network: &mut Network) -> Result<()> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open DYD file: {}", path.display()))?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line.unwrap_or_default();
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(model) = try_parse_dyd_line(trimmed) {
            network.dyd_models.push(model);
        }
    }

    network.dyd_generators = extract_dyd_generators(&network.dyd_models);

    Ok(())
}

fn try_parse_dyd_line(line: &str) -> Option<DydModelData> {
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("netting")
        || lower.starts_with("lodrep")
        || lower == "models"
        || lower.starts_with("title")
        || lower.starts_with("comments")
    {
        return None;
    }

    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 6 {
        return None;
    }

    let first = tokens[0].as_str();
    if !first.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    // Skip section-like tokens
    if matches!(
        first.to_ascii_lowercase().as_str(),
        "program" | "end" | "header"
    ) {
        return None;
    }

    let bus: u32 = tokens[1].parse().ok()?;
    if bus == 0 {
        return None;
    }

    let colon_pos = tokens.iter().position(|t| t == ":")?;
    let name = tokens.get(2).map(String::as_str).unwrap_or("").into();
    let kv: f64 = tokens.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let id = tokens.get(4).map(String::as_str).unwrap_or("1").into();

    let mut params = Vec::new();
    for token in tokens.iter().skip(colon_pos + 1) {
        if token.starts_with('#') {
            continue;
        }
        if let Some((_, v)) = token.split_once('=') {
            if let Ok(val) = v.parse::<f64>() {
                params.push(val);
            }
            continue;
        }
        if let Ok(v) = token.parse::<f64>() {
            params.push(v);
        }
    }

    Some(DydModelData {
        model_type: first.into(),
        bus,
        name,
        kv,
        id,
        params,
        raw_line: line.into(),
    })
}

fn extract_dyd_generators(models: &[DydModelData]) -> Vec<DydGeneratorData> {
    let mut out = Vec::new();
    for m in models {
        let fam = m.model_type.to_ascii_uppercase();
        let is_ibr = fam.contains("REPC")
            || fam.contains("REPCA")
            || fam.contains("IBR")
            || fam.contains("PV")
            || fam.contains("WIND");
        if fam.contains("GENROU") || fam.contains("GENSAL") || fam.contains("REPC") || is_ibr {
            out.push(DydGeneratorData {
                bus_id: m.bus,
                id: m.id.clone(),
                mva: m.params.first().copied().unwrap_or(0.0),
                model_family: m.model_type.clone(),
                is_ibr,
            });
        }
    }
    out
}
