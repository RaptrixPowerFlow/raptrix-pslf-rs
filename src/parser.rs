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

use crate::models::{Bus, DydGeneratorData, DydModelData, Generator, Load, Network, Transformer2W};

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

    let mut current_section = String::new();
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
            current_section = "title".to_string();
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
            current_section = "solution parameters".to_string();
            i = parse_solution_parameters(&lines, i + 1, &mut network)?;
            continue;
        }

        if lower.starts_with("substation data") {
            current_section = "substation".to_string();
            i = skip_section(&lines, i + 1)?;
            continue;
        }

        if lower.starts_with("bus data") {
            current_section = "bus".to_string();
            i = parse_bus_data(&lines, i + 1, &mut network.buses)?;
            continue;
        }

        if lower.starts_with("load data") {
            current_section = "load".to_string();
            i = parse_load_data(&lines, i + 1, &mut network.loads)?;
            continue;
        }

        if lower.starts_with("generator data") {
            current_section = "generator".to_string();
            i = parse_generator_data(&lines, i + 1, &mut network.generators)?;
            continue;
        }

        if lower.starts_with("branch data") {
            current_section = "branch".to_string();
            i = parse_branch_data(&lines, i + 1, &mut network.branches)?;
            continue;
        }

        if lower.starts_with("transformer data") {
            current_section = "transformer".to_string();
            i = parse_transformer_data(&lines, i + 1, &mut network.transformers, &mut network.transformers_3w)?;
            continue;
        }

        // Skip other sections for now (area data, owner data, switched shunt, dc, etc.)
        if is_known_section_header(line) {
            current_section = lower.split_whitespace().next().unwrap_or("unknown").to_string();
        }

        i += 1;
    }

    Ok(network)
}

fn is_known_section_header(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.starts_with("title") ||
    l.starts_with("comments") ||
    l.starts_with("solution parameters") ||
    l.starts_with("substation data") ||
    l.starts_with("bus data") ||
    l.starts_with("load data") ||
    l.starts_with("generator data") ||
    l.starts_with("branch data") ||
    l.starts_with("transformer data") ||
    l.starts_with("area data") ||
    l.starts_with("owner data") ||
    l.starts_with("switched shunt") ||
    l.starts_with("dc ") ||
    l.starts_with("end")
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

fn parse_solution_parameters(lines: &[String], mut start: usize, net: &mut Network) -> Result<usize> {
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

fn parse_bus_data(lines: &[String], mut start: usize, buses: &mut Vec<Bus>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }

        // Example line (from real file):
        //      1  "EDNA 1"    : 29.198000 -96.661600 0.190000  " "
        // Bus records in "bus data" section are denser.
        // We do a best-effort parse; real production parser will be more robust.
        if let Some(bus) = parse_one_bus_line(line) {
            buses.push(bus);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_bus_line(line: &str) -> Option<Bus> {
    // Improved parser for the real "bus data" format seen in the Texas EPC files.
    // Typical line after header:
    //   110001 "EDNA 1 1    " 138.0000 " "  0  :  1 1.037093  1.037093  -4.242394    7    1 ...
    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 4 {
        return None;
    }

    let number: u32 = tokens[0].parse().ok()?;
    let name = tokens.get(1).map(String::as_str).unwrap_or("").into();

    // Find the numeric voltage / angle fields after the colon or after name+kv
    let mut bus = Bus {
        number,
        name,
        kv: tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        ..Default::default()
    };

    // Look for the pattern after the colon or the type/volt/angle triplet
    for (i, tok) in tokens.iter().enumerate() {
        if *tok == ":" && i + 3 < tokens.len() {
            bus.ty = tokens.get(i + 1).and_then(|s| s.parse::<u8>().ok()).unwrap_or(1);
            bus.volt = tokens.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            bus.angle = tokens.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            break;
        }
    }

    // Fallback: scan for plausible pu voltage and angle values
    if bus.volt < 0.1 {
        for tok in &tokens {
            if let Ok(v) = tok.parse::<f64>() {
                if (0.5..=1.5).contains(&v) && bus.volt < 0.1 {
                    bus.volt = v;
                } else if v.abs() < 180.0 && bus.angle == 0.0 {
                    bus.angle = v;
                }
            }
        }
    }

    Some(bus)
}

fn parse_load_data(lines: &[String], mut start: usize, loads: &mut Vec<Load>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(load) = parse_one_load_line(line) {
            loads.push(load);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_load_line(line: &str) -> Option<Load> {
    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 3 { return None; }

    Some(Load {
        bus: tokens[0].parse().ok()?,
        id: tokens.get(1).map(String::as_str).unwrap_or("").into(),
        p: tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        q: tokens.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        status: 1,
    })
}

fn parse_generator_data(lines: &[String], mut start: usize, gens: &mut Vec<Generator>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        if let Some(generator) = parse_one_generator_line(line) {
            gens.push(generator);
        }
        start += 1;
    }
    Ok(start)
}

fn parse_one_generator_line(line: &str) -> Option<Generator> {
    let tokens = tokenize_pslf_line(line);
    if tokens.len() < 4 { return None; }

    Some(Generator {
        bus: tokens[0].parse().ok()?,
        id: tokens.get(1).map(String::as_str).unwrap_or("1").into(),
        pg: tokens.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        qg: tokens.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        status: 1,
        ..Default::default()
    })
}

fn parse_branch_data(lines: &[String], mut start: usize, branches: &mut Vec<crate::models::Branch>) -> Result<usize> {
    while start < lines.len() {
        let line = lines[start].trim();
        if line.is_empty() || line.starts_with('!') || is_known_section_header(line) {
            return Ok(start);
        }
        // Simplified branch parsing for now
        let tokens = tokenize_pslf_line(line);
        if tokens.len() >= 4 {
            if let (Ok(f), Ok(t)) = (tokens[0].parse::<u32>(), tokens[1].parse::<u32>()) {
                branches.push(crate::models::Branch {
                    from_bus: f,
                    to_bus: t,
                    ckt: tokens.get(2).map(String::as_str).unwrap_or("1").into(),
                    status: 1,
                    ..Default::default()
                });
            }
        }
        start += 1;
    }
    Ok(start)
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
        let tokens = tokenize_pslf_line(line);
        if tokens.len() >= 4 {
            if let (Ok(f), Ok(t)) = (tokens[0].parse::<u32>(), tokens[1].parse::<u32>()) {
                // Heuristic: if we see tertiary-like fields later in the line, treat as potential 3W
                let has_tertiary = line.to_ascii_lowercase().contains("tert") || tokens.len() > 12;
                if has_tertiary {
                    t3w.push(crate::models::Transformer3W {
                        bus_h: f,
                        bus_m: t,
                        bus_l: 0, // will be improved
                        ckt: tokens.get(2).map(String::as_str).unwrap_or("1").into(),
                        status: 1,
                        ..Default::default()
                    });
                } else {
                    t2w.push(Transformer2W {
                        from_bus: f,
                        to_bus: t,
                        ckt: tokens.get(2).map(String::as_str).unwrap_or("1").into(),
                        status: 1,
                        ..Default::default()
                    });
                }
            }
        }
        start += 1;
    }
    Ok(start)
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
            ' ' | '\t' | ':' if !in_quotes => {
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

    let mut current_model: Option<DydModelData> = None;

    for line in reader.lines() {
        let line = line.unwrap_or_default();
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();

        // Common dynamic model starters in PSLF DYD
        if lower.starts_with("genrou") ||
           lower.starts_with("repc_a") ||
           lower.starts_with("esst") ||
           lower.starts_with("ggov") ||
           lower.starts_with("ieeest") ||
           lower.starts_with("lodrep") ||
           lower.starts_with("netting")
        {
            if let Some(model) = current_model.take() {
                network.dyd_models.push(model);
            }

            let tokens = tokenize_pslf_line(trimmed);
            if tokens.len() >= 5 {
                let model_type = tokens[0].clone().into();
                let bus: u32 = tokens[1].parse().unwrap_or(0);
                let name = tokens.get(2).map(String::as_str).unwrap_or("").into();
                let kv: f64 = tokens.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let id = tokens.get(4).map(String::as_str).unwrap_or("1").into();

                let mut params = Vec::new();
                if let Some(colon_pos) = trimmed.find(':') {
                    let after = &trimmed[colon_pos + 1..];
                    for p in after.split_whitespace() {
                        if let Ok(v) = p.parse::<f64>() {
                            params.push(v);
                        }
                    }
                }

                current_model = Some(DydModelData {
                    model_type,
                    bus,
                    name,
                    kv,
                    id,
                    params,
                    raw_line: trimmed.into(),
                });
            }
        }
    }

    if let Some(model) = current_model {
        network.dyd_models.push(model);
    }

    // Extract generator-level IBR info (very similar to psse DYR logic)
    network.dyd_generators = extract_dyd_generators(&network.dyd_models);

    Ok(())
}

fn extract_dyd_generators(models: &[DydModelData]) -> Vec<DydGeneratorData> {
    let mut out = Vec::new();
    for m in models {
        let fam = m.model_type.to_ascii_uppercase();
        let is_ibr = fam.contains("REPC") || fam.contains("REPCA") || fam.contains("IBR") || fam.contains("PV") || fam.contains("WIND");
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
