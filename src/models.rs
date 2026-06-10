// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! PSLF data model (EPC + DYD records).
//! Field names kept close to GE PSLF documentation for fidelity and future C++ porting.
//! This is the direct analogue of raptrix-psse-rs/src/models.rs.

use std::fmt;

#[derive(Debug, Default)]
pub struct Network {
    /// Case title / description (from the leading "title" section).
    pub title: Box<str>,
    /// System MVA base (from "solution parameters" sbase).
    pub sbase: f64,

    // Core power-flow records (populated by the EPC parser)
    pub buses: Vec<Bus>,
    pub loads: Vec<Load>,
    pub generators: Vec<Generator>,
    pub branches: Vec<Branch>,
    pub transformers: Vec<Transformer2W>,
    pub transformers_3w: Vec<Transformer3W>,
    pub fixed_shunts: Vec<FixedShunt>,
    pub switched_shunts: Vec<SwitchedShunt>,
    /// Derived from switched shunts at export time (PSSE-style bank rows).
    pub switched_shunt_banks: Vec<SwitchedShuntBankRow>,
    pub areas: Vec<Area>,
    pub owners: Vec<Owner>,
    pub zones: Vec<Zone>, // often synthetic / derived

    // DC (present in header but frequently empty in the reference cases)
    pub dc_buses: Vec<DcBus>,
    pub dc_converters: Vec<DcConverter>,

    // Dynamics (populated when a .dyd is supplied)
    pub dyd_models: Vec<DydModelData>,
    pub dyd_generators: Vec<DydGeneratorData>,
}

// ---------------------------------------------------------------------------
// Bus / Substation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Bus {
    pub number: u32,
    pub name: Box<str>,
    pub kv: f64,
    pub ty: u8, // bus type code (1=PQ, 2=PV, 3=Slack, etc.)
    pub vsched: f64,
    pub volt: f64,  // pu
    pub angle: f64, // degrees
    pub area: u32,
    pub zone: u32,
    pub owner: u32,
    // Latitude / longitude from "substation data" when present
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            number: 0,
            name: "".into(),
            kv: 0.0,
            ty: 1,
            vsched: 1.0,
            volt: 1.0,
            angle: 0.0,
            area: 0,
            zone: 0,
            owner: 0,
            lat: None,
            lon: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Load {
    pub bus: u32,
    pub id: Box<str>,
    pub p: f64,
    pub q: f64,
    pub status: u8,
    // ZIP or other characteristic flags from DYD / load data when present
}

// ---------------------------------------------------------------------------
// Generator (Machine)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Generator {
    pub bus: u32,
    pub id: Box<str>,
    pub pg: f64,
    pub qg: f64,
    pub qt: f64,
    pub qb: f64,
    pub vs: f64,
    pub ireg: u32, // remote regulation bus (0 = local)
    pub mbase: f64,
    pub status: u8,
    pub pt: f64,
    pub pb: f64,
    // Additional long_id / reg_name fields from EPC
    pub long_id: Box<str>,
}

// ---------------------------------------------------------------------------
// Branch (non-transformer)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Branch {
    pub from_bus: u32,
    pub to_bus: u32,
    pub ckt: Box<str>,
    /// Per-unit on system base (same convention as PSS/E RAW R/X/B).
    pub r: f64,
    pub x: f64,
    pub b: f64,
    pub rate_a: f64,
    pub rate_b: f64,
    pub rate_c: f64,
    pub status: u8,
    pub from_name: Box<str>,
    pub to_name: Box<str>,
}

// ---------------------------------------------------------------------------
// 2-Winding Transformer (including legs of 3W when expanded)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Transformer2W {
    pub from_bus: u32,
    pub to_bus: u32,
    pub ckt: Box<str>,
    pub r: f64,
    pub x: f64,
    pub b: f64,
    pub rate_a: f64,
    pub rate_b: f64,
    pub rate_c: f64,
    pub status: u8,
    pub tap: f64,
    pub phase_shift: f64,
    pub from_kv: f64,
    pub to_kv: f64,
    pub tertiary_present: bool, // set true when the EPC row carried tertiary data
}

// ---------------------------------------------------------------------------
// 3-Winding Transformer (native representation when EPC provides it)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Transformer3W {
    pub bus_h: u32,
    pub bus_m: u32,
    pub bus_l: u32,
    pub ckt: Box<str>,
    pub status: u8,
    pub r_hm: f64,
    pub x_hm: f64,
    pub r_ml: f64,
    pub x_ml: f64,
    pub r_lh: f64,
    pub x_lh: f64,
    pub rate_h: f64,
    pub rate_m: f64,
    pub rate_l: f64,
    pub tap_h: f64,
    pub tap_m: f64,
    pub tap_l: f64,
    pub phase_shift_deg: f64,
    pub nominal_kv_h: f64,
    pub nominal_kv_m: f64,
    pub nominal_kv_l: f64,
    pub star_bus_id: u32, // may be synthetic or explicit in the deck
}

// ---------------------------------------------------------------------------
// Shunts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct FixedShunt {
    pub bus: u32,
    pub id: Box<str>,
    pub g: f64,
    pub b: f64,
    pub status: u8,
}

#[derive(Debug, Clone, Default)]
pub struct SwitchedShunt {
    pub bus: u32,
    pub id: Box<str>,
    pub modsw: u8,
    pub status: u8,
    /// Voltage lower limit (pu).
    pub vswlo: f64,
    /// Voltage upper limit (pu).
    pub vswhi: f64,
    /// Initial switched susceptance (pu on system base) from EPC SVD `b` field.
    pub b_init: f64,
    /// Compact (N, B) bank pairs from SVD continuation lines (`B` in pu per step).
    pub bank_pairs: Vec<(u32, f64)>,
    /// Flat per-step susceptance values (pu), expanded from bank_pairs.
    pub steps: Vec<f64>,
}

/// Export row for the `switched_shunt_banks` table.
#[derive(Debug, Clone, Default)]
pub struct SwitchedShuntBankRow {
    pub shunt_id: i32,
    pub bank_id: i32,
    pub b_mvar: f64,
    pub status: bool,
    pub step: i32,
}

// ---------------------------------------------------------------------------
// Area / Owner / Zone (minimal for metadata + ownership tables)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Area {
    pub number: u32,
    pub name: Box<str>,
    pub desired_net_interchange: f64,
    pub swing_bus: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Owner {
    pub number: u32,
    pub name: Box<str>,
}

#[derive(Debug, Clone, Default)]
pub struct Zone {
    pub number: u32,
    pub name: Box<str>,
}

// ---------------------------------------------------------------------------
// DC (stubs for completeness; most reference cases have zero records)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct DcBus {
    pub number: u32,
    pub name: Box<str>,
}

#[derive(Debug, Clone, Default)]
pub struct DcConverter {
    pub id: Box<str>,
}

// ---------------------------------------------------------------------------
// Dynamics (DYD)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DydModelData {
    pub model_type: Box<str>, // "genrou", "repc_a", "esst4b", "ggov1", ...
    pub bus: u32,
    pub name: Box<str>,
    pub kv: f64,
    pub id: Box<str>,
    pub params: Vec<f64>,   // raw numeric parameters after the colon
    pub raw_line: Box<str>, // full original line for provenance
}

#[derive(Debug, Clone, Default)]
pub struct DydGeneratorData {
    pub bus_id: u32,
    pub id: Box<str>,
    pub mva: f64,
    pub model_family: Box<str>, // "GENROU" / "REPC" / etc. for IBR classification
    pub is_ibr: bool,
}

impl fmt::Display for DydModelData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} @ bus {} id={}", self.model_type, self.bus, self.id)
    }
}
