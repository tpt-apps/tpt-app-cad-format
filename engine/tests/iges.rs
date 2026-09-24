//! Known-good / known-malformed sample validation for the IGES path, plus
//! parser/writer round-trip and column-layout guarantees.

use std::fs;

use tpt_cad_engine::model::Severity;
use tpt_cad_engine::{
    iges::{parse_iges, write_iges, IgesParam},
    validate, validate_iges, CadFormat, FindingKind,
};

fn sample(name: &str) -> Vec<u8> {
    fs::read(format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/{}"),
        name
    ))
    .unwrap_or_else(|e| panic!("sample {name} missing: {e}"))
}

#[test]
fn detects_iges_by_layout() {
    assert_eq!(
        CadFormat::detect(&sample("known-good.igs")),
        Some(CadFormat::Iges)
    );
    assert_eq!(
        CadFormat::detect(&sample("known-malformed.igs")),
        Some(CadFormat::Iges)
    );
}

#[test]
fn every_sample_record_is_exactly_80_columns() {
    // The fixture padding is the most error-prone part — verify it
    // programmatically instead of trusting the generator.
    for name in ["known-good.igs", "known-malformed.igs"] {
        let text = String::from_utf8(sample(name)).unwrap();
        for (i, line) in text.lines().enumerate() {
            assert_eq!(
                line.chars().count(),
                80,
                "{name} record {} is {} columns",
                i + 1,
                line.chars().count()
            );
        }
    }
}

#[test]
fn known_good_iges_is_clean() {
    let report = validate(&sample("known-good.igs"));
    assert_eq!(report.format, Some(CadFormat::Iges));
    assert_eq!(report.stats.schema.as_deref(), Some("TPT-Tools / 1.0.0"));
    assert_eq!(report.stats.entity_count, 4, "4 directory entries expected");
    assert_eq!(report.stats.line_count, 18, "2 S + 3 G + 8 D + 4 P + 1 T");
    assert_eq!(
        report.stats.entity_types,
        vec![
            ("Circular Arc".to_string(), 1),
            ("Color Definition".to_string(), 1),
            ("Line".to_string(), 1),
            ("Point".to_string(), 1),
        ]
    );
    assert!(
        report.findings.is_empty(),
        "clean file must have zero findings, got: {report:?}"
    );
    assert!(report.is_valid());
}

#[test]
fn iges_parser_reads_params_fields_and_hollerith() {
    let file = parse_iges(&sample("known-good.igs"));
    assert_eq!(file.start.len(), 2);
    assert_eq!(&file.globals[..2], ["1H,", "1H;"]);
    assert_eq!(file.globals.len(), 23, "2 delimiter fields + 21 data fields");
    assert_eq!(file.entities.len(), 4);

    let point = &file.entities[0];
    assert_eq!(point.de_number, 1);
    assert_eq!(point.de_type, 116);
    assert_eq!(point.param_pointer, 1);
    assert_eq!(point.fields[0][0], "116");
    // Directory fields are stored trimmed of padding: the 8-digit status
    // carries no blanks, so it reads back verbatim.
    assert_eq!(point.fields[0][8], "00000000");
    assert_eq!(point.fields[1][7], "POINT");
    assert_eq!(
        point.params,
        vec![
            IgesParam::Real(50.0),
            IgesParam::Real(50.0),
            IgesParam::Real(0.0),
            IgesParam::Int(0),
        ]
    );

    // The colour name is a Hollerith string containing BOTH delimiter
    // characters — the field splitter must honour the 7H... length.
    let color = &file.entities[3];
    assert_eq!(color.de_type, 314);
    assert_eq!(color.params[0], IgesParam::Str("A,B;CDE".to_string()));

    let pointers: Vec<u64> = file.entities.iter().map(|e| e.param_pointer).collect();
    assert_eq!(pointers, vec![1, 2, 3, 4]);
    let types: Vec<u64> = file.entities.iter().map(|e| e.de_type).collect();
    assert_eq!(types, vec![116, 110, 100, 314]);
}

#[test]
fn malformed_iges_reports_every_planted_issue() {
    let report = validate(&sample("known-malformed.igs"));
    assert_eq!(report.format, Some(CadFormat::Iges));
    assert!(!report.is_valid());

    let has = |kind: &FindingKind, sev: Severity, needle: &str| {
        report
            .findings
            .iter()
            .any(|f| &f.kind == kind && f.severity == sev && f.message.contains(needle))
    };

    // Planted D sequence gap: D sequence 4 is missing.
    assert!(
        has(&FindingKind::SequenceGap, Severity::Error, "D section sequence number is 5, expected 4"),
        "D sequence gap must be reported, got: {report:?}"
    );
    // E4 points at P record 99; the section holds 5 records.
    assert!(
        has(&FindingKind::DanglingReference, Severity::Error, "parameter record 99"),
        "P pointer beyond the section must be reported, got: {report:?}"
    );
    // E3's second D record claims type 101 against 100.
    assert!(
        has(&FindingKind::MalformedEntity, Severity::Error, "101 does not match first record type 100"),
        "D line-2 type mismatch must be reported, got: {report:?}"
    );
    // E5 declares 3 parameter records but its data spans 1.
    assert!(
        has(&FindingKind::MalformedEntity, Severity::Error, "declares 3 parameter record(s)"),
        "wrong declared P line count must be reported, got: {report:?}"
    );
    // E2 is a zero-length line.
    assert!(
        has(&FindingKind::DegenerateGeometry, Severity::Warning, "zero-length line"),
        "zero-length line must be reported, got: {report:?}"
    );
    // E6's P record names directory entry 12 instead of 11.
    assert!(
        has(&FindingKind::DanglingReference, Severity::Error, "directory entry 12"),
        "dangling DE pointer in P must be reported, got: {report:?}"
    );
    // Terminator counts 2/2/10/4/1 against actual 1/3/12/5/1.
    assert!(
        report.findings.iter().filter(|f| f.kind == FindingKind::SequenceGap
            && f.message.contains("terminator counts")).count() == 4,
        "all four wrong terminator counts must be reported, got: {report:?}"
    );

    // The degenerate line is attributed to entity #2, a Line.
    let zero = report
        .findings
        .iter()
        .find(|f| f.message.contains("zero-length line"))
        .expect("zero-length line finding");
    assert_eq!(zero.entity.as_deref(), Some("Line"));
    assert_eq!(zero.reference, Some(2));
    assert!(zero.line.is_some(), "finding must carry a file line");
}

#[test]
fn sequence_gap_detected_by_mutation() {
    let text = String::from_utf8(sample("known-good.igs")).unwrap();
    assert!(text.contains("S      2"));
    let mutated = text.replace("S      2", "S      3");
    let report = validate_iges(mutated.as_bytes());
    assert!(
        report.findings.iter().any(|f| f.kind == FindingKind::SequenceGap
            && f.severity == Severity::Error
            && f.message.contains("S section sequence number is 3, expected 2")),
        "S sequence gap must be reported, got: {report:?}"
    );
}

#[test]
fn short_record_is_flagged_with_line_number() {
    let good = sample("known-good.igs");
    let cut = &good[..good.len() - 4];
    let report = validate_iges(cut);
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == FindingKind::MalformedEntity && f.message.contains("77 column"))
        .unwrap_or_else(|| panic!("short final record must be flagged, got: {report:?}"));
    assert_eq!(finding.line, Some(18), "the truncated terminator is line 18");
    assert!(!report.is_valid());
}

#[test]
fn empty_bytes_report_missing_section() {
    let report = validate_iges(b"");
    assert_eq!(report.findings.len(), 1, "exactly one finding, got: {report:?}");
    assert_eq!(report.findings[0].kind, FindingKind::MissingSection);
    assert_eq!(report.findings[0].severity, Severity::Error);
    assert!(report.findings[0].message.contains("no IGES records"));
    assert!(!report.is_valid());
}

#[test]
fn non_iges_bytes_are_not_parseable() {
    let report = validate_iges(b"this is definitely not an IGES file");
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::NotParseable && f.severity == Severity::Error),
        "bytes without the IGES layout must be NotParseable, got: {report:?}"
    );
    assert!(!report.is_valid());
}

#[test]
fn writer_emits_exactly_80_column_records() {
    let file = parse_iges(&sample("known-good.igs"));
    let written = write_iges(&file);
    let text = String::from_utf8(written.clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 18, "2 S + 3 G + 8 D + 4 P + 1 T");
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(
            line.chars().count(),
            80,
            "written record {} is not 80 columns",
            i + 1
        );
    }
    assert_eq!(
        CadFormat::detect(&written),
        Some(CadFormat::Iges),
        "written bytes must re-detect as IGES"
    );
}

#[test]
fn round_trip_is_structurally_equal() {
    let original = parse_iges(&sample("known-good.igs"));
    let reparsed = parse_iges(&write_iges(&original));
    assert_eq!(
        original, reparsed,
        "parse(write(parse(x))) must equal parse(x)"
    );
}

#[test]
fn written_file_revalidates_clean() {
    let file = parse_iges(&sample("known-good.igs"));
    let report = validate_iges(&write_iges(&file));
    assert!(
        report.error_count() == 0 && report.warning_count() == 0 && report.info_count() == 0,
        "the written file must revalidate clean, got: {report:?}"
    );
    assert_eq!(report.stats.entity_count, 4);
    assert_eq!(report.stats.schema.as_deref(), Some("TPT-Tools / 1.0.0"));
    assert_eq!(report.stats.line_count, 18);
}

// ---------- synthetic edge cases ----------

fn rec(content: &str, letter: char, seq: u64) -> String {
    let mut c: String = content.chars().take(72).collect();
    while c.chars().count() < 72 {
        c.push(' ');
    }
    format!("{c}{letter}{seq:>7}\n")
}

fn d8(v: u64) -> String {
    format!("{v:>8}")
}

#[test]
fn synthetic_edge_cases_surface_as_expected() {
    let mut iges = String::new();
    iges.push_str(&rec("synthetic edge-case file", 'S', 1));
    // 14 Global fields in one record: delimiters, sender, file, system,
    // preprocessor, bits, magnitudes, significance, scale, unit flag, units.
    iges.push_str(&rec(
        "1H,,1H;,2HX,2HY,2HZ,5H1.0.0,32,38,6,308,15,2HX,1.0,2,2HMM;",
        'G',
        1,
    ));
    // E1: 126 B-spline with only 3 parameters (too few to validate).
    // E2: vendor type 10050. E3: broken Hollerith length. E4: unterminated
    // parameter data.
    for (i, (t1, t2, label)) in [
        (126u64, 126u64, "BSPLINE"),
        (10050, 10050, "VENDOR"),
        (116, 116, "POINT"),
        (116, 116, "POINT"),
    ]
    .into_iter()
    .enumerate()
    {
        let seq = (i as u64) * 2 + 1;
        let label = format!("{label:<8}");
        iges.push_str(&rec(
            &format!(
                "{}{}{}{}{}{}{}{}{}",
                d8(t1),
                d8(i as u64 + 1),
                d8(0),
                d8(0),
                d8(0),
                d8(0),
                d8(0),
                d8(0),
                "00000000"
            ),
            'D',
            seq,
        ));
        iges.push_str(&rec(
            &format!(
                "{}{}{}{}{}{}{}{}{}",
                d8(t2),
                d8(1),
                d8(1),
                d8(1),
                d8(0),
                "        ",
                "        ",
                label,
                d8(1)
            ),
            'D',
            seq + 1,
        ));
    }
    iges.push_str(&rec(&format!("{}{}{}", d8(126), d8(1), "1.0,2.0,3.0;"), 'P', 1));
    iges.push_str(&rec(&format!("{}{}{}", d8(10050), d8(3), "42;"), 'P', 2));
    // Declares 5 characters but the body ends after 2 — broken length, and
    // the record delimiter is swallowed with it (also unterminated).
    iges.push_str(&rec(&format!("{}{}{}", d8(116), d8(5), "5HAB;"), 'P', 3));
    iges.push_str(&rec(&format!("{}{}{}", d8(116), d8(7), "1.0,2.0,3.0"), 'P', 4));
    iges.push_str(&rec(&format!("{}{}{}{}{}", d8(1), d8(1), d8(8), d8(4), d8(1)), 'T', 1));

    let report = validate_iges(iges.as_bytes());
    assert_eq!(report.format, Some(CadFormat::Iges));

    let has = |sev: Severity, needle: &str| {
        report
            .findings
            .iter()
            .any(|f| f.severity == sev && f.message.contains(needle))
    };
    // 126 with < 6 parameters: conservative "cannot validate" warning.
    assert!(
        has(Severity::Warning, "too few to validate the B-spline"),
        "short B-spline must be reported, got: {report:?}"
    );
    // Type 10050 sits in the vendor range: informational only.
    assert!(
        has(Severity::Info, "vendor-specific entity type 10050"),
        "vendor type must be reported, got: {report:?}"
    );
    // `5HAB;` declares 5 characters but only 2 are present.
    assert!(
        has(Severity::Error, "broken Hollerith string"),
        "bad Hollerith length must be reported, got: {report:?}"
    );
    // E3's record delimiter was swallowed by the over-long Hollerith, and
    // E4's data never reaches one at all.
    assert_eq!(
        report.findings.iter().filter(|f| f.severity == Severity::Error).count(),
        3,
        "broken Hollerith plus two unterminated entities expected, got: {report:?}"
    );
    assert!(!report.findings.iter().any(|f| f.message.contains("non-standard entity type")));
    assert_eq!(report.stats.entity_count, 4);
}
