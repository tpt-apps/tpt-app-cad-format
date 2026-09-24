//! TPT CAD Format engine: STEP (ISO 10303-21) / IGES 5.3 structural
//! validation, and — under the `pro` feature — cross-format batch conversion.
//!
//! Everything here is plain `std` Rust with no wasm/UI dependencies, so the
//! correctness moat is host-testable: `cargo test -p tpt-cad-engine` runs the
//! known-good/known-malformed sample suite before any browser is involved.
//!
//! The STEP parser/lexer comes from `tpt-io-ifc` (a format-agnostic STEP
//! Physical File reader). This crate owns everything upstream does not
//! provide: the finding/issue model, semantic checks (dangling references,
//! malformed entities, degenerate geometry), the IGES reader, and the writers
//! used for Pro conversion.

pub mod iges;
pub mod model;
pub mod step;

#[cfg(feature = "pro")]
pub mod convert;

pub use iges::validate_iges;
pub use model::{CadError, CadFormat, FileStats, Finding, FindingKind, Severity, ValidationReport};
pub use step::validate_step;

/// Inspect raw file bytes: detect the format and run its validator.
///
/// Files that are neither STEP nor IGES yield a single `UnsupportedFormat`
/// error finding rather than a hard error, so callers can always render a
/// report.
pub fn validate(bytes: &[u8]) -> ValidationReport {
    match CadFormat::detect(bytes) {
        Some(CadFormat::Step) => step::validate_step(bytes),
        Some(CadFormat::Iges) => iges::validate_iges(bytes),
        None => ValidationReport {
            format: None,
            stats: FileStats {
                line_count: bytes.iter().filter(|&&b| b == b'\n').count() + !bytes.is_empty() as usize,
                ..FileStats::default()
            },
            findings: vec![Finding {
                kind: FindingKind::UnsupportedFormat,
                severity: Severity::Error,
                entity: None,
                reference: None,
                line: None,
                message: "Not a recognisable STEP (ISO 10303-21) or IGES file — no ISO-10303-21 \
                          magic and no IGES section layout found."
                    .to_string(),
            }],
        },
    }
}
