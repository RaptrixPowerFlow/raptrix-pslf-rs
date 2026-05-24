// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! CLI entry-point for `raptrix-pslf-rs`.
//!
//! Subcommands mirror raptrix-psse-rs for maximum user and developer familiarity:
//! * `convert`  — parse GE PSLF .epc (and optional .dyd) → .rpf
//! * `view`     — pretty-print an existing .rpf summary
//! * `validate` — basic structural checks on an .epc file (no output written)

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "raptrix-pslf-rs")]
#[command(author = "Raptrix PowerFlow")]
#[command(version, about = "GE PSLF (.epc + .dyd) to Raptrix PowerFlow Interchange converter", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Parse a GE PSLF case and write a Raptrix PowerFlow Interchange (.rpf) file.
    ///
    /// Example:
    ///   raptrix-pslf-rs convert --epc tests/networks/Texas7k_20210804.EPC --output case.rpf
    Convert {
        /// Path to the GE PSLF EPC file (.epc or .EPC).
        #[arg(long)]
        epc: PathBuf,

        /// Optional path to the GE PSLF dynamic data file (.dyd).
        #[arg(long)]
        dyd: Option<PathBuf>,

        /// Output path for the Raptrix PowerFlow Interchange file (.rpf).
        #[arg(long)]
        output: PathBuf,

        /// Transformer representation mode for 3-winding devices (mirrors psse-rs).
        #[arg(long, default_value = "native-3w")]
        transformer_mode: String,

        #[arg(long)]
        study_purpose: Option<String>,

        #[arg(long = "scenario-tag")]
        scenario_tags: Vec<String>,

        #[arg(long)]
        case_mode: Option<String>,

        #[arg(long)]
        default_shunt_control_mode: Option<String>,
    },

    /// Pretty-print a Raptrix PowerFlow Interchange (.rpf) file summary.
    View {
        #[arg(long)]
        input: PathBuf,
    },

    /// Run basic structural checks on a PSLF .epc file (no output written).
    Validate {
        #[arg(long)]
        epc: PathBuf,

        #[arg(long, default_value_t = false)]
        strict: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Convert {
            epc,
            dyd,
            output,
            transformer_mode,
            study_purpose,
            scenario_tags,
            case_mode,
            default_shunt_control_mode,
        } => {
            let epc_str = epc
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("EPC path is not valid UTF-8"))?;
            if let Some(d) = &dyd {
                if let Some(ds) = d.to_str() {
                    eprintln!("[raptrix-pslf-rs] DYD file: {ds}");
                }
            }
            let out_str = output
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("output path is not valid UTF-8"))?;

            let transformer_representation_mode =
                raptrix_pslf_rs::TransformerRepresentationMode::from_cli_value(&transformer_mode)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "invalid --transformer-mode '{}'; expected one of: expanded, native-3w",
                            transformer_mode
                        )
                    })?;

            let export_options = raptrix_pslf_rs::ExportOptions {
                transformer_representation_mode,
                study_purpose,
                scenario_tags,
                case_mode_override: case_mode,
                default_shunt_control_mode_override: default_shunt_control_mode,
                ..Default::default()
            };

            // Placeholder — real implementation comes in later phases
            raptrix_pslf_rs::write_pslf_to_rpf_with_options(
                epc_str,
                dyd.as_deref().and_then(|p| p.to_str()),
                out_str,
                &export_options,
            )?;

            let summary = raptrix_cim_arrow::summarize_rpf(&output)?;
            eprintln!(
                "[raptrix-pslf-rs] Wrote {} — {} tables, {} total rows",
                output.display(),
                summary.tables.len(),
                summary.total_rows,
            );
            for t in &summary.tables {
                eprintln!("  {:30} {:6} rows", t.table_name, t.rows);
            }
        }

        Commands::View { input } => {
            let summary = raptrix_cim_arrow::summarize_rpf(&input)?;
            println!("RPF file: {}", input.display());
            println!(
                "  tables: {}  total rows: {}  all canonical: {}",
                summary.tables.len(),
                summary.total_rows,
                summary.has_all_canonical_tables,
            );
            for t in &summary.tables {
                println!("  {:30} {:6} rows", t.table_name, t.rows);
            }
        }

        Commands::Validate { epc, strict } => {
            let epc_str = epc
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("EPC path is not valid UTF-8"))?;
            let report = raptrix_pslf_rs::validate_pslf_epc(epc_str)?;
            report.print_summary();
            if strict && !report.is_clean() {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
