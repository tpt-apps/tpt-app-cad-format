//! IGES 5.x structural validation, parsing, and canonical re-emission.
//!
//! IGES is fixed-format: 80-column records with a section letter in column 73
//! and a right-justified sequence number in columns 74-80. Files are divided
//! into Start (S), Global (G), Directory Entry (D, two records per entity),
//! Parameter Data (P), and Terminator (T) sections.
//!
//! The module is split in three: `parse` frames the records and builds the
//! structural `IgesFile` the Pro converter consumes, `checks` runs the
//! semantic validators over it, and `write` serialises an `IgesFile` back to
//! canonical fixed-column IGES.

mod checks;
mod parse;
mod write;

pub use parse::{parse_iges, IgesEntity, IgesFile, IgesParam};
pub use write::write_iges;

use std::collections::BTreeMap;

use crate::model::{CadFormat, FileStats, Finding, ValidationReport};

/// Findings of one class are capped so a pathological file still renders a
/// usable report (the Pro detailed report raises this per-file budget).
/// Mirror of [`crate::step::MAX_PER_KIND`].
pub const MAX_PER_KIND: usize = 50;

/// Validate raw IGES bytes and produce the findings report.
///
/// Never panics and always returns a report: bytes that do not carry the
/// IGES record layout at all surface as a single `NotParseable` error.
pub fn validate_iges(bytes: &[u8]) -> ValidationReport {
    let line_count = String::from_utf8_lossy(bytes).lines().count();
    let (file, layout, mut findings, framed) = parse::parse_with_layout(bytes);
    if framed {
        checks::run(&file, &layout, &mut findings);
    }

    let stats = FileStats {
        format: Some(CadFormat::Iges),
        schema: schema_label(&file),
        entity_count: file.entities.len(),
        entity_types: type_histogram(&file),
        line_count,
    };
    let mut report = ValidationReport { format: Some(CadFormat::Iges), stats, findings };
    report.sort();
    report
}

/// Collect findings, capped per [`FindingKind`] so pathological files still
/// render (same policy as the STEP validator).
pub(crate) struct Sink<'a> {
    findings: &'a mut Vec<Finding>,
}

impl<'a> Sink<'a> {
    pub(crate) fn new(findings: &'a mut Vec<Finding>) -> Self {
        Sink { findings }
    }

    pub(crate) fn push(&mut self, finding: Finding) {
        if self.findings.iter().filter(|f| f.kind == finding.kind).count() < MAX_PER_KIND {
            self.findings.push(finding);
        }
    }
}

/// Standard IGES entity type numbers this edition names. Numbers outside the
/// table but inside 100-500 are histogrammed as "Type N"; anything else gets
/// an informational finding. The table is deliberately conservative — when
/// unsure, a number is omitted and simply reads as "Type N".
pub(crate) fn entity_type_name(t: u64) -> Option<&'static str> {
    match t {
        100 => Some("Circular Arc"),
        102 => Some("Composite Curve"),
        104 => Some("Conic Arc"),
        106 => Some("Copious Data"),
        108 => Some("Plane"),
        110 => Some("Line"),
        112 => Some("Parametric Spline Curve"),
        114 => Some("Parametric Spline Surface"),
        116 => Some("Point"),
        118 => Some("Ruled Surface"),
        120 => Some("Surface of Revolution"),
        122 => Some("Tabulated Cylinder"),
        124 => Some("Transformation Matrix"),
        126 => Some("Rational B-spline Curve"),
        128 => Some("Rational B-spline Surface"),
        140 => Some("Offset Curve"),
        141 => Some("Boundary Entity"),
        142 => Some("Curve on Surface"),
        144 => Some("Trimmed Surface"),
        150 => Some("Block"),
        152 => Some("Right Angular Wedge"),
        154 => Some("Right Circular Cylinder"),
        156 => Some("Right Circular Cone Frustum"),
        158 => Some("Sphere"),
        160 => Some("Torus"),
        162 => Some("Solid of Revolution"),
        164 => Some("Solid of Linear Extrusion"),
        166 => Some("Boolean Tree"),
        212 => Some("General Note"),
        214 => Some("Leader (Arrow)"),
        216 => Some("Linear Dimension"),
        218 => Some("Angular Dimension"),
        220 => Some("Diameter Dimension"),
        222 => Some("Radius Dimension"),
        228 => Some("General Symbol"),
        230 => Some("Sectional Area"),
        308 => Some("Subfigure Definition"),
        310 => Some("Subfigure Instance"),
        314 => Some("Color Definition"),
        316 => Some("Line Weight Definition"),
        320 => Some("Network Subfigure Definition"),
        322 => Some("Attribute Table Definition"),
        402 => Some("Associativity Form"),
        404 => Some("Drawing"),
        406 => Some("Property"),
        408 => Some("Singular Subfigure Instance"),
        410 => Some("View"),
        412 => Some("Rectangular Array"),
        414 => Some("Circular Array"),
        415 => Some("External Reference"),
        420 => Some("Network Subfigure Instance"),
        _ => None,
    }
}

/// Display name for findings and the histogram: the table name when known,
/// "Type N" otherwise.
pub(crate) fn display_name(t: u64) -> String {
    match entity_type_name(t) {
        Some(name) => name.to_string(),
        None => format!("Type {t}"),
    }
}

/// Compact one-line schema description: `{native system} / {preprocessor
/// version}` (Global fields 5 and 6), falling back to the units name.
fn schema_label(file: &IgesFile) -> Option<String> {
    let g = &file.globals;
    let field =
        |idx: usize| -> String { g.get(idx).map(|tok| parse::decoded(tok)).unwrap_or_default() };
    let native = field(4);
    let preprocessor = field(5);
    match (native.is_empty(), preprocessor.is_empty()) {
        (false, false) => Some(format!("{native} / {preprocessor}")),
        (false, true) => Some(native),
        (true, false) => Some(preprocessor),
        (true, true) => {
            let units = field(14);
            if units.is_empty() { None } else { Some(units) }
        }
    }
}

fn type_histogram(file: &IgesFile) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &file.entities {
        *counts.entry(display_name(e.de_type)).or_default() += 1;
    }
    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rows
}
