// raptrix-pslf-rs
// Copyright (c) 2026 Raptrix PowerFlow
//
// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

//! Minimal structural validation for PSLF EPC files (opt-in via `validate` subcommand).
//! Mirrors the design of psse-rs validation.rs (Severity + Report) but without
//! MMWG-specific rules for v1.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Info => "INFO ",
            Severity::Warning => "WARN ",
            Severity::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.issues.iter().all(|i| i.severity != Severity::Error)
    }

    pub fn print_summary(&self) {
        if self.issues.is_empty() {
            eprintln!("[raptrix-pslf-rs] validation: no issues found");
            return;
        }
        eprintln!(
            "[raptrix-pslf-rs] validation: {} issue(s)",
            self.issues.len()
        );
        for issue in &self.issues {
            eprintln!(
                "  {} [{}] {}",
                issue.severity.label(),
                issue.code,
                issue.message
            );
        }
    }
}

pub fn run_basic_checks(_network: &crate::models::Network) -> ValidationReport {
    // Placeholder — real checks added after parser is functional.
    ValidationReport::default()
}
