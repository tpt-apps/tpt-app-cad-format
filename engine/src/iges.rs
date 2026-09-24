//! IGES 5.x structural validation.
//!
//! IGES is fixed-format: 80-column records with a section letter in column 73
//! and a right-justified sequence number in columns 74-80. Files are divided
//! into Start (S), Global (G), Directory Entry (D, two records per entity),
//! Parameter Data (P), and Terminator (T) sections.
//!
//! STATUS: placeholder while the full parser lands (Phase 1). The STEP path
//! is complete; this stub keeps the workspace green so the free build can
//! ship before IGES validation is wired in.

use crate::model::{CadFormat, FileStats, Finding, FindingKind, Severity, ValidationReport};

/// Validate raw IGES bytes and produce the findings report.
pub fn validate_iges(bytes: &[u8]) -> ValidationReport {
    let text = String::from_utf8_lossy(bytes);
    let findings = vec![Finding {
        kind: FindingKind::UnsupportedConstruct,
        severity: Severity::Info,
        entity: None,
        reference: None,
        line: None,
        message: "IGES structural validation is not wired into this build yet; the file was \
                  recognised but not analysed."
            .to_string(),
    }];
    ValidationReport {
        format: Some(CadFormat::Iges),
        stats: FileStats {
            format: Some(CadFormat::Iges),
            line_count: text.lines().count(),
            ..FileStats::default()
        },
        findings,
    }
}
