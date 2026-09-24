//! Semantic checks over the parsed IGES structure: Global sanity, D/P
//! cross-references, degenerate geometry on the common CAD entities, and
//! Terminator consistency.

use std::collections::HashSet;

use super::parse::{decoded, Layout};
use super::{display_name, entity_type_name, IgesFile, IgesParam, Sink};
use crate::model::{Finding, FindingKind, Severity};

pub(crate) fn run(file: &IgesFile, layout: &Layout, findings: &mut Vec<Finding>) {
    let mut sink = Sink::new(findings);
    check_globals(file, layout, &mut sink);
    check_directory(file, layout, &mut sink);
    check_params(file, layout, &mut sink);
    check_geometry(file, layout, &mut sink);
    check_types(file, &mut sink);
    check_terminator(layout, &mut sink);
}

fn check_globals(file: &IgesFile, layout: &Layout, sink: &mut Sink<'_>) {
    if layout.global_line.is_none() {
        return; // missing Global already reported while parsing
    }
    let at = layout.global_line.unwrap_or(1);
    let g = &file.globals;
    if g.len() < 14 {
        sink.push(
            Finding::new(
                FindingKind::MalformedEntity,
                Severity::Warning,
                format!(
                    "Global section has {} field(s), expected at least 14 — readers will \
                     improvise units and precision",
                    g.len()
                ),
            )
            .at(at),
        );
    }
    let present = |idx: usize| -> Option<String> {
        g.get(idx).map(|tok| decoded(tok)).filter(|s| !s.trim().is_empty())
    };
    if let Some(bits) = present(6) {
        match bits.trim().parse::<i64>() {
            Ok(v) if v > 0 => {}
            Ok(v) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    format!("Global bits-per-integer is {v}, expected a positive count"),
                )
                .at(at),
            ),
            Err(_) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    format!("Global bits-per-integer '{bits}' is not a positive integer"),
                )
                .at(at),
            ),
        }
    }
    if let Some(scale) = present(12) {
        let promoted = scale.trim().replace(['D', 'd'], "E");
        match promoted.parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => {}
            _ => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    format!("Global model space scale '{scale}' is not a positive real"),
                )
                .at(at),
            ),
        }
    }
    if let Some(flag) = present(13) {
        match flag.trim().parse::<i64>() {
            Ok(v) if (1..=11).contains(&v) => {}
            Ok(v) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    format!(
                        "Global unit flag {v} is outside 1-11 — importing systems cannot \
                         resolve the units"
                    ),
                )
                .at(at),
            ),
            Err(_) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    format!("Global unit flag '{flag}' is not an integer in 1-11"),
                )
                .at(at),
            ),
        }
    }
}

/// Parameter Data pointer range: every D entry must point inside the P
/// section. (The pointer/DE mismatch checks live in `check_params`.)
fn check_directory(file: &IgesFile, layout: &Layout, sink: &mut Sink<'_>) {
    for (i, e) in file.entities.iter().enumerate() {
        let Some(span) = layout.spans.get(i) else { continue };
        if !span.pointer_ok {
            continue; // non-numeric pointer already reported
        }
        let ptr = usize::try_from(e.param_pointer).unwrap_or(usize::MAX);
        if ptr < 1 || ptr > layout.param_records.len() {
            let name = display_name(e.de_type);
            let line = layout.dir_lines.get(2 * i).copied().unwrap_or(1);
            sink.push(
                Finding::new(
                    FindingKind::DanglingReference,
                    Severity::Error,
                    format!(
                        "directory entry {} points at parameter record {}, but the Parameter \
                         Data section holds only {} record(s)",
                        e.de_number,
                        e.param_pointer,
                        layout.param_records.len()
                    ),
                )
                .at(line)
                .on_entity(name.as_str(), e.de_number),
            );
        }
    }
}

fn check_params(file: &IgesFile, layout: &Layout, sink: &mut Sink<'_>) {
    for (i, e) in file.entities.iter().enumerate() {
        let Some(span) = layout.spans.get(i) else { continue };
        let in_range = span.pointer_ok
            && e.param_pointer >= 1
            && usize::try_from(e.param_pointer).is_ok_and(|p| p <= layout.param_records.len());
        if !in_range {
            continue; // pointer problems already reported
        }
        let name = display_name(e.de_type);
        if span.records.is_empty() {
            // The P record at the pointer belongs to a different entity.
            let ptr = usize::try_from(e.param_pointer).unwrap_or(usize::MAX);
            let rec = &layout.param_records[ptr - 1];
            let found =
                rec.de.map(|v| v.to_string()).unwrap_or_else(|| format!("'{}'", rec.de_raw));
            sink.push(
                Finding::new(
                    FindingKind::DanglingReference,
                    Severity::Error,
                    format!(
                        "parameter record {} references directory entry {found}, but entity {} \
                         starts at directory record {}",
                        rec.seq,
                        e.de_number,
                        2 * e.de_number - 1
                    ),
                )
                .at(rec.line)
                .on_entity(name.as_str(), e.de_number),
            );
        } else if span.declared_ok && span.declared != span.records.len() as u64 {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {} declares {} parameter record(s) but its data spans {}",
                        e.de_number,
                        span.declared,
                        span.records.len()
                    ),
                )
                .at(span.first_line.unwrap_or(1))
                .on_entity(name.as_str(), e.de_number),
            );
        }
        for &j in &span.records {
            let rec = &layout.param_records[j];
            match rec.etype {
                Some(t) if t == e.de_type => {}
                Some(t) => sink.push(
                    Finding::new(
                        FindingKind::MalformedEntity,
                        Severity::Error,
                        format!(
                            "parameter record {} carries entity type {t}, but directory entry \
                             {} is a {name}",
                            rec.seq, e.de_number
                        ),
                    )
                    .at(rec.line)
                    .on_entity(name.as_str(), e.de_number),
                ),
                None => sink.push(
                    Finding::new(
                        FindingKind::MalformedEntity,
                        Severity::Error,
                        format!(
                            "parameter record {} has a non-numeric entity type '{}'",
                            rec.seq, rec.etype_raw
                        ),
                    )
                    .at(rec.line)
                    .on_entity(name.as_str(), e.de_number),
                ),
            }
        }
    }
}

fn check_geometry(file: &IgesFile, layout: &Layout, sink: &mut Sink<'_>) {
    for (i, e) in file.entities.iter().enumerate() {
        let name = display_name(e.de_type);
        let line = layout.dir_lines.get(2 * i).copied().unwrap_or(1);
        let de = e.de_number;
        match e.de_type {
            100 => {
                flag_non_finite(sink, line, &name, de, &e.params);
                if let Some(v) = first_reals(&e.params, 7) {
                    // Params: ZT, centre X/Y, start X/Y, end X/Y.
                    if v[3] == v[1] && v[4] == v[2] {
                        push_geom(
                            sink,
                            Severity::Warning,
                            line,
                            &name,
                            de,
                            format!(
                                "directory entry {de} ({name}): arc has zero radius — start \
                                 point equals the centre"
                            ),
                        );
                    }
                }
            }
            108 => {
                flag_non_finite(sink, line, &name, de, &e.params);
                if let Some(v) = first_reals(&e.params, 4) {
                    if v[0] == 0.0 && v[1] == 0.0 && v[2] == 0.0 {
                        push_geom(
                            sink,
                            Severity::Warning,
                            line,
                            &name,
                            de,
                            format!(
                                "directory entry {de} ({name}): plane has no normal — the A, B \
                                 and C coefficients are all zero"
                            ),
                        );
                    }
                }
            }
            110 => {
                flag_non_finite(sink, line, &name, de, &e.params);
                if let Some(v) = first_reals(&e.params, 6) {
                    if v[0] == v[3] && v[1] == v[4] && v[2] == v[5] {
                        push_geom(
                            sink,
                            Severity::Warning,
                            line,
                            &name,
                            de,
                            format!(
                                "directory entry {de} ({name}): zero-length line — start and \
                                 end points coincide"
                            ),
                        );
                    }
                }
            }
            116 => {
                flag_non_finite(sink, line, &name, de, &e.params);
            }
            126 => {
                if e.params.len() < 6 {
                    sink.push(
                        Finding::new(
                            FindingKind::UnsupportedConstruct,
                            Severity::Warning,
                            format!(
                                "directory entry {de} ({name}): only {} parameter(s) — too few \
                                 to validate the B-spline",
                                e.params.len()
                            ),
                        )
                        .at(line)
                        .on_entity(name.as_str(), de),
                    );
                }
                flag_non_finite(sink, line, &name, de, &e.params);
            }
            _ => {}
        }
    }
}

fn push_geom(
    sink: &mut Sink<'_>,
    severity: Severity,
    line: usize,
    name: &str,
    de: u64,
    text: String,
) {
    sink.push(Finding::new(FindingKind::DegenerateGeometry, severity, text).at(line).on_entity(name, de));
}

/// NaN/inf in any parsed numeric parameter of a geometry-carrying entity.
fn flag_non_finite(sink: &mut Sink<'_>, line: usize, name: &str, de: u64, params: &[IgesParam]) {
    if params.iter().any(|p| matches!(p, IgesParam::Real(v) if !v.is_finite())) {
        push_geom(
            sink,
            Severity::Error,
            line,
            name,
            de,
            format!(
                "directory entry {de} ({name}): contains a NaN/inf parameter — importing \
                 systems cannot place this geometry"
            ),
        );
    }
}

/// The first `count` parameters as reals, or `None` when there are fewer
/// than `count` or any of them is not numeric (Blank/Str) — such entities
/// are left unjudged.
fn first_reals(params: &[IgesParam], count: usize) -> Option<Vec<f64>> {
    if params.len() < count {
        return None;
    }
    params
        .iter()
        .take(count)
        .map(|p| match p {
            IgesParam::Real(v) => Some(*v),
            IgesParam::Int(v) => Some(*v as f64),
            _ => None,
        })
        .collect()
}

fn check_types(file: &IgesFile, sink: &mut Sink<'_>) {
    let mut reported: HashSet<u64> = HashSet::new();
    for e in &file.entities {
        let t = e.de_type;
        if t == 0 || entity_type_name(t).is_some() || !reported.insert(t) {
            continue;
        }
        if t >= 10000 {
            sink.push(
                Finding::new(
                    FindingKind::UnsupportedConstruct,
                    Severity::Info,
                    format!(
                        "vendor-specific entity type {t} — the 10000-99999 range is reserved \
                         for implementer extensions"
                    ),
                )
                .on_entity(display_name(t).as_str(), e.de_number),
            );
        } else if !(100..=500).contains(&t) {
            sink.push(
                Finding::new(
                    FindingKind::UnsupportedConstruct,
                    Severity::Info,
                    format!(
                        "non-standard entity type {t} — outside the standard IGES entity \
                         number ranges"
                    ),
                )
                .on_entity(display_name(t).as_str(), e.de_number),
            );
        }
    }
}

fn check_terminator(layout: &Layout, sink: &mut Sink<'_>) {
    let Some(term) = &layout.term else {
        sink.push(
            Finding::new(
                FindingKind::MissingSection,
                Severity::Error,
                "no Terminator section — importing systems treat the file as truncated",
            ),
        );
        return;
    };
    let sections =
        ["Start (S)", "Global (G)", "Directory Entry (D)", "Parameter Data (P)", "Terminator (T)"];
    for (k, name) in sections.iter().enumerate() {
        match term.counts[k].parse::<u64>() {
            Ok(v) if v == layout.actual[k] => {}
            Ok(v) => sink.push(
                Finding::new(
                    FindingKind::SequenceGap,
                    Severity::Error,
                    format!(
                        "terminator counts {v} {name} record(s), the file contains {}",
                        layout.actual[k]
                    ),
                )
                .at(term.line),
            ),
            Err(_) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!("terminator {name} count '{}' is non-numeric", term.counts[k]),
                )
                .at(term.line),
            ),
        }
    }
}
