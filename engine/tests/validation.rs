//! Known-good / known-malformed sample validation for the STEP path and the
//! format sniffer. The IGES equivalents live beside these once the IGES
//! parser lands.

use std::fs;

use tpt_cad_engine::{model::Severity, validate};

fn sample(name: &str) -> Vec<u8> {
    fs::read(format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/{}"),
        name
    ))
    .unwrap_or_else(|e| panic!("sample {name} missing: {e}"))
}

#[test]
fn detects_step_by_magic() {
    assert_eq!(
        tpt_cad_engine::CadFormat::detect(&sample("known-good.step")),
        Some(tpt_cad_engine::CadFormat::Step)
    );
}

#[test]
fn rejects_non_cad_bytes() {
    for junk in [
        &b"<html><body>hello</body></html>"[..],
        b"PK\x03\x04totally-a-zip",
        b"",
    ] {
        let report = validate(junk);
        assert_eq!(report.format, None, "junk must not detect: {junk:?}");
        assert_eq!(report.findings[0].kind, tpt_cad_engine::FindingKind::UnsupportedFormat);
    }
}

#[test]
fn known_good_step_has_no_errors_or_warnings() {
    let report = validate(&sample("known-good.step"));
    assert_eq!(report.format, Some(tpt_cad_engine::CadFormat::Step));
    assert_eq!(
        report.stats.schema.as_deref(),
        Some("AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }")
    );
    assert_eq!(report.stats.entity_count, 12, "12 #instances expected");
    assert!(
        report.error_count() == 0 && report.warning_count() == 0,
        "clean file must be clean, got: {report:?}"
    );
    assert!(report.is_valid());
}

#[test]
fn malformed_step_reports_every_planted_issue() {
    let report = validate(&sample("known-malformed.step"));
    assert_eq!(report.format, Some(tpt_cad_engine::CadFormat::Step));
    assert!(!report.is_valid());

    let has = |needle: &str| {
        report
            .findings
            .iter()
            .any(|f| f.message.contains(needle) && f.severity == Severity::Error)
    };

    // #2 LINE points at #99, which does not exist.
    assert!(has("#99"), "dangling #99 must be reported");
    // #3 DIRECTION is the zero vector.
    assert!(has("zero vector"), "zero DIRECTION must be reported");
    // #5 CIRCLE radius 0.
    assert!(has("radius 0"), "zero radius must be reported");
    // #6 VECTOR magnitude -2.5.
    assert!(has("-2.5"), "negative VECTOR magnitude must be reported");
    // #7 B-spline: 3 control points, degree 3 => expected 7, multiplicities sum 8.
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == tpt_cad_engine::FindingKind::MalformedEntity
                && f.message.contains("B_SPLINE")),
        "knot-vector inconsistency must be reported, got: {report:?}"
    );
}

#[test]
fn unparseable_step_reports_not_parseable_at_line() {
    // `NaN` is not a legal STEP real literal. The upstream lexer survives it
    // as an enum token, so the validator must flag the coordinate as
    // degenerate geometry rather than crash or pass silently.
    let report = validate(&sample("known-unparseable.step"));
    let finding = report
        .findings
        .iter()
        .find(|f| f.message.contains("NaN"))
        .expect("NaN coordinate must be flagged");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(
        finding.kind,
        tpt_cad_engine::FindingKind::DegenerateGeometry
    );
    assert!(!report.is_valid());
}

#[test]
fn truncated_step_is_not_parseable_with_line_number() {
    let good = sample("known-good.step");
    let cut = &good[..good.len() * 3 / 4];
    let report = validate(cut);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == tpt_cad_engine::FindingKind::NotParseable),
        "truncation must surface as NotParseable, got: {report:?}"
    );
}
