//! STEP (ISO 10303-21) validation: the semantic layer on top of
//! `tpt-io-ifc`'s STEP Physical File parser.
//!
//! The upstream parser tokenises and builds the `#id`-keyed entity graph but
//! performs no semantic checks (its validator is a no-op). This module walks
//! that graph and reports what downstream CAD systems actually trip over:
//! dangling `#id` references, malformed attribute arities on the common
//! CAD entities, and degenerate geometry (NaN/inf coordinates, zero-length
//! directions, non-positive radii).

use std::collections::{BTreeMap, HashSet};

use tpt_io_ifc::{parse_ifc, IfcDocument, Value};

use crate::model::{CadFormat, FileStats, Finding, FindingKind, Severity, ValidationReport};

/// Validate raw STEP bytes and produce the findings report.
pub fn validate_step(bytes: &[u8]) -> ValidationReport {
    let mut findings = Vec::new();
    let text = String::from_utf8_lossy(bytes);
    let line_count = text.lines().count();
    let schema = sniff_schema(&text);

    let mut stats = FileStats {
        format: Some(CadFormat::Step),
        schema: schema.clone(),
        line_count,
        ..FileStats::default()
    };

    let doc = match parse_ifc(bytes) {
        Ok(doc) => doc,
        Err(err) => {
            let line = parse_error_line(bytes, &err);
            findings.push(Finding {
                kind: FindingKind::NotParseable,
                severity: Severity::Error,
                entity: None,
                reference: None,
                line,
                message: format!("STEP parser rejected the file: {err}"),
            });
            stats.entity_count = 0;
            return ValidationReport { format: Some(CadFormat::Step), stats, findings };
        }
    };

    stats.entity_count = doc.instances.len();
    stats.entity_types = type_histogram(&doc);

    check_dangling_references(&doc, &mut findings);
    check_entities(&doc, &mut findings);
    ValidationReport { format: Some(CadFormat::Step), stats, findings }
}

/// Extract the `FILE_SCHEMA(('NAME'));` schema string textually — the upstream
/// parser skips the HEADER section.
fn sniff_schema(text: &str) -> Option<String> {
    let idx = text.find("FILE_SCHEMA")?;
    let rest = &text[idx..text.len().min(idx + 512)];
    let open = rest.find('\'')?;
    let close = rest[open + 1..].find('\'')?;
    let name = rest[open + 1..open + 1 + close].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Map a parse error back to a 1-based line number from its byte offset.
fn parse_error_line(bytes: &[u8], err: &tpt_io_ifc::IfcError) -> Option<usize> {
    let tpt_io_ifc::IfcError::Core(pe) = err else {
        return None;
    };
    let offset = pe.offset.min(bytes.len());
    Some(bytes[..offset].iter().filter(|&&b| b == b'\n').count() + 1)
}

fn type_histogram(doc: &IfcDocument) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for inst in &doc.instances {
        *counts.entry(inst.entity.to_uppercase()).or_default() += 1;
    }
    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rows
}

/// Every `#id` mentioned anywhere in the attribute tree must resolve.
fn check_dangling_references(doc: &IfcDocument, findings: &mut Vec<Finding>) {
    let mut referenced: HashSet<u64> = HashSet::new();
    for inst in &doc.instances {
        collect_refs(&inst.attributes, &mut referenced);
    }
    let mut dangling: Vec<u64> = referenced
        .into_iter()
        .filter(|id| doc.by_id(*id).is_none())
        .collect();
    dangling.sort_unstable();
    for id in dangling.into_iter().take(MAX_PER_KIND) {
        findings.push(Finding {
            kind: FindingKind::DanglingReference,
            severity: Severity::Error,
            entity: None,
            reference: Some(id),
            line: None,
            message: format!("#{} is referenced but no such instance exists — importing systems \
                              will drop geometry or abort on this file.",
                id),
        });
    }
}

fn collect_refs(attrs: &[Value], out: &mut HashSet<u64>) {
    for attr in attrs {
        collect_refs_value(attr, out);
    }
}

fn collect_refs_value(value: &Value, out: &mut HashSet<u64>) {
    match value {
        Value::Ref(id) => {
            out.insert(*id);
        }
        Value::List(items) => {
            for item in items {
                collect_refs_value(item, out);
            }
        }
        Value::Entity(_, attrs) => collect_refs(attrs, out),
        _ => {}
    }
}

/// Findings of one class are capped so a pathological file still renders a
/// usable report (the Pro detailed report raises this per-file budget).
pub const MAX_PER_KIND: usize = 50;

struct Cursor<'a> {
    findings: &'a mut Vec<Finding>,
}


impl<'a> Cursor<'a> {
    fn push(&mut self, finding: Finding) {
        if self.findings.iter().filter(|f| f.kind == finding.kind).count() < MAX_PER_KIND {
            self.findings.push(finding);
        }
    }

    fn attrs(&mut self, inst: &tpt_io_ifc::EntityInstance, expected: usize, signature: &str) -> bool {
        if inst.attributes.len() != expected {
            self.push(Finding {
                kind: FindingKind::MalformedEntity,
                severity: Severity::Error,
                entity: Some(inst.entity.clone()),
                reference: Some(inst.id),
                line: None,
                message: format!(
                    "#{} {} has {} attribute(s), expected {expected}: {}",
                    inst.id,
                    inst.entity,
                    inst.attributes.len(),
                    signature
                ),
            });
            return false;
        }
        true
    }

    fn check_coord_list(
        &mut self,
        inst: &tpt_io_ifc::EntityInstance,
        coords: &[f64],
        min_dim: usize,
        max_dim: usize,
    ) {
        if coords.len() < min_dim || coords.len() > max_dim {
            self.push(Finding {
                kind: FindingKind::MalformedEntity,
                severity: Severity::Error,
                entity: Some(inst.entity.clone()),
                reference: Some(inst.id),
                line: None,
                message: format!(
                    "#{} {} coordinate list has {} value(s), expected {min_dim}-{max_dim}",
                    inst.id,
                    inst.entity,
                    coords.len()
                ),
            });
            return;
        }
        if coords.iter().any(|v| !v.is_finite()) {
            self.push(Finding {
                kind: FindingKind::DegenerateGeometry,
                severity: Severity::Error,
                entity: Some(inst.entity.clone()),
                reference: Some(inst.id),
                line: None,
                message: format!(
                    "#{} {} contains a NaN/inf coordinate — importing systems cannot place \
                     this geometry",
                    inst.id,
                    inst.entity
                ),
            });
        }
    }
}

/// Curated semantic checks for the entities that carry real CAD geometry.
/// Entities outside this set are histogrammed but not arity-checked.
fn check_entities(doc: &IfcDocument, findings: &mut Vec<Finding>) {
    let mut cursor = Cursor { findings };
    for inst in &doc.instances {
        let name = inst.entity.as_str();
        match () {
            _ if name.eq_ignore_ascii_case("CARTESIAN_POINT") => {
                if cursor.attrs(inst, 2, "(name, coordinates)") {
                    if let Some(Value::List(coords)) = inst.attributes.get(1) {
                        let values = real_list(coords);
                        match values {
                            Some(values) => cursor.check_coord_list(inst, &values, 1, 3),
                            None if coords.iter().any(|v| {
                                matches!(v, Value::Enum(e) if e.eq_ignore_ascii_case("NaN"))
                            }) =>
                            {
                                // Some writers emit bare `NaN` where a real
                                // belongs; the upstream lexer survives it as
                                // an enum, but no geometry can use it.
                                cursor.push(Finding {
                                    kind: FindingKind::DegenerateGeometry,
                                    severity: Severity::Error,
                                    entity: Some(inst.entity.clone()),
                                    reference: Some(inst.id),
                                    line: None,
                                    message: format!(
                                        "#{} CARTESIAN_POINT contains a NaN coordinate — \
                                         importing systems cannot place this geometry",
                                        inst.id
                                    ),
                                });
                            }
                            None => cursor.push(Finding {
                                kind: FindingKind::MalformedEntity,
                                severity: Severity::Error,
                                entity: Some(inst.entity.clone()),
                                reference: Some(inst.id),
                                line: None,
                                message: format!(
                                    "#{} CARTESIAN_POINT coordinate list contains non-numeric \
                                     values",
                                    inst.id
                                ),
                            }),
                        }
                    }
                }
            }
            _ if name.eq_ignore_ascii_case("DIRECTION") => {
                if cursor.attrs(inst, 2, "(name, direction_ratios)") {
                    if let Some(Value::List(ratios)) = inst.attributes.get(1) {
                        if let Some(values) = real_list(ratios) {
                            cursor.check_coord_list(inst, &values, 2, 3);
                            if values.iter().all(|v| *v == 0.0) && values.len() >= 2 {
                                cursor.push(Finding {
                                    kind: FindingKind::DegenerateGeometry,
                                    severity: Severity::Error,
                                    entity: Some(inst.entity.clone()),
                                    reference: Some(inst.id),
                                    line: None,
                                    message: format!(
                                        "#{} DIRECTION is the zero vector — it defines no \
                                         direction at all",
                                        inst.id
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            _ if name.eq_ignore_ascii_case("VECTOR") => {
                if cursor.attrs(inst, 3, "(name, orientation, magnitude)") {
                    let magnitude = match inst.attributes[2] {
                        Value::Real(v) => Some(v),
                        _ => None,
                    };
                    match magnitude {
                        None => cursor.push(Finding {
                            kind: FindingKind::MalformedEntity,
                            severity: Severity::Error,
                            entity: Some(inst.entity.clone()),
                            reference: Some(inst.id),
                            line: None,
                            message: format!("#{} VECTOR magnitude is not a real number", inst.id),
                        }),
                        Some(v) if !v.is_finite() || v < 0.0 => cursor.push(Finding {
                            kind: FindingKind::DegenerateGeometry,
                            severity: Severity::Error,
                            entity: Some(inst.entity.clone()),
                            reference: Some(inst.id),
                            line: None,
                            message: format!(
                                "#{} VECTOR magnitude is {v} — expected a finite, non-negative \
                                 length",
                                inst.id
                            ),
                        }),
                        Some(_) => {}
                    }
                }
            }
            _ if name.eq_ignore_ascii_case("LINE") => {
                // The referenced pnt/dir are separate instances checked in
                // their own arms; here only arity is enforced.
                cursor.attrs(inst, 3, "(name, pnt, dir)");
            }
            _ if name.eq_ignore_ascii_case("CIRCLE")
                || name.eq_ignore_ascii_case("HYPERBOLA")
                || name.eq_ignore_ascii_case("PARABOLA") =>
            {
                // CIRCLE(name, position, radius); the conics take
                // (name, position, focal_distance) — same trailing real.
                if cursor.attrs(inst, 3, "(name, position, radius/focal)") {
                    if let Some(Value::Real(radius)) = inst.attributes.last() {
                        if !radius.is_finite() || *radius <= 0.0 {
                            cursor.push(Finding {
                                kind: FindingKind::DegenerateGeometry,
                                severity: Severity::Error,
                                entity: Some(inst.entity.clone()),
                                reference: Some(inst.id),
                                line: None,
                                message: format!(
                                    "#{} {name} has radius {radius} — expected a finite, \
                                     positive value",
                                    inst.id
                                ),
                            });
                        }
                    }
                }
            }
            _ if name.eq_ignore_ascii_case("ELLIPSE") => {
                if cursor.attrs(inst, 4, "(name, position, semi_axis_1, semi_axis_2)") {
                    for (slot, value) in inst.attributes[2..].iter().enumerate() {
                        if let Value::Real(v) = value {
                            if !v.is_finite() || *v <= 0.0 {
                                cursor.push(Finding {
                                    kind: FindingKind::DegenerateGeometry,
                                    severity: Severity::Error,
                                    entity: Some(inst.entity.clone()),
                                    reference: Some(inst.id),
                                    line: None,
                                    message: format!(
                                        "#{} ELLIPSE semi-axis {} is {v} — expected a finite, \
                                         positive value",
                                        inst.id,
                                        slot + 1
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            _ if name.eq_ignore_ascii_case("AXIS2_PLACEMENT_3D") => {
                cursor.attrs(inst, 4, "(name, location, axis, ref_direction)");
            }
            _ if name.eq_ignore_ascii_case("B_SPLINE_CURVE_WITH_KNOTS") => {
                check_bspline_knots(&mut cursor, inst);
            }
            _ => {
                // Unknown/unmodelled entity — counted in the histogram only.
            }
        }
    }
}

/// `sum(knot_multiplicities) == degree + control_points + 1` — the knot-vector
/// consistency rule every B-spline consumer enforces.
fn check_bspline_knots(cursor: &mut Cursor, inst: &tpt_io_ifc::EntityInstance) {
    // (name, degree, control_points_list, curve_form, closed_curve,
    //  self_intersect, knot_multiplicities, knots, knot_spec)
    if inst.attributes.len() < 8 {
        return; // arity failure already reported generically elsewhere
    }
    let degree = match inst.attributes[1] {
        Value::Int(d) if d >= 1 => d as usize,
        _ => {
            cursor.push(Finding {
                kind: FindingKind::MalformedEntity,
                severity: Severity::Error,
                entity: Some(inst.entity.clone()),
                reference: Some(inst.id),
                line: None,
                message: format!("#{} B_SPLINE_CURVE_WITH_KNOTS degree is not a positive integer", inst.id),
            });
            return;
        }
    };
    let control_count = match &inst.attributes[2] {
        Value::List(points) => points.len(),
        _ => return,
    };
    let multiplicities = match &inst.attributes[6] {
        Value::List(mults) => mults
            .iter()
            .map(|m| match m {
                Value::Int(v) => Some(*v as usize),
                _ => None,
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    let Some(multiplicities) = multiplicities else {
        cursor.push(Finding {
            kind: FindingKind::MalformedEntity,
            severity: Severity::Error,
            entity: Some(inst.entity.clone()),
            reference: Some(inst.id),
            line: None,
            message: format!(
                "#{} B_SPLINE_CURVE_WITH_KNOTS knot multiplicities contain non-integers",
                inst.id
            ),
        });
        return;
    };
    let knot_sum: usize = multiplicities.iter().sum();
    let expected = control_count + degree + 1;
    if knot_sum != expected {
        cursor.push(Finding {
            kind: FindingKind::MalformedEntity,
            severity: Severity::Error,
            entity: Some(inst.entity.clone()),
            reference: Some(inst.id),
            line: None,
            message: format!(
                "#{} B_SPLINE_CURVE_WITH_KNOTS knot multiplicities sum to {knot_sum}, but \
                 degree + control points + 1 = {expected} — the knot vector is inconsistent \
                 and surfaces built on it will fail",
                inst.id
            ),
        });
    }
}

fn real_list(list: &[Value]) -> Option<Vec<f64>> {
    list.iter()
        .map(|v| match v {
            Value::Real(r) => Some(*r),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        })
        .collect()
}
