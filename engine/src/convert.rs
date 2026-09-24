//! Pro edition: cross-format conversion and canonical normalization.
//!
//! STEP and IGES are enormous formats; this module converts the geometry
//! subset that has a real counterpart in both worlds — points (STEP
//! CARTESIAN_POINT ↔ IGES 116), lines (LINE ↔ 110), circles/arcs (CIRCLE
//! ↔ 100) — and normalizes files to their own format (STEP→STEP, IGES→IGES)
//! through a full parse/re-emit cycle. Entity types without a counterpart
//! are reported in [`ConversionResult::skipped`] instead of being silently
//! dropped, and the desktop shell writes that list into the conversion
//! report beside the output file.
//!
//! Same-format conversion is lossless for anything the parsers accept
//! (canonical re-serialization). Cross-format conversion is deliberately
//! conservative: a tilted circle needs a transformation-matrix entity chain
//! (124) and B-splines need full control-point/knot translation — those are
//! future work, listed as skipped.

use tpt_io_ifc::{parse_ifc, Value};

use crate::iges::{self, IgesEntity, IgesFile, IgesParam};
use crate::model::{CadError, CadFormat};

/// One conversion's outcome.
#[derive(Debug, Clone)]
pub struct ConversionResult {
    /// Converted file bytes (canonical form of the target format).
    pub output: Vec<u8>,
    pub output_format: CadFormat,
    /// Entities successfully mapped/normalized.
    pub converted_entities: usize,
    /// Entity types with no counterpart in the target format (or that were
    /// degenerate), as `"<type> ×<count>"` rows, most common first.
    pub skipped: Vec<String>,
}

/// Convert (or normalize) raw CAD file bytes to `to`.
pub fn convert(input: &[u8], to: CadFormat) -> Result<ConversionResult, CadError> {
    let Some(from) = CadFormat::detect(input) else {
        return Err(CadError::UnsupportedFormat);
    };
    match (from, to) {
        (CadFormat::Step, CadFormat::Step) => normalize_step(input),
        (CadFormat::Iges, CadFormat::Iges) => normalize_iges(input),
        (CadFormat::Step, CadFormat::Iges) => step_to_iges(input),
        (CadFormat::Iges, CadFormat::Step) => iges_to_step(input),
    }
}

// ---------------------------------------------------------------------------
// Same-format normalization
// ---------------------------------------------------------------------------

fn normalize_step(input: &[u8]) -> Result<ConversionResult, CadError> {
    let doc = parse_ifc(input).map_err(|_| CadError::Invalid("unparseable STEP file"))?;
    let mut out = String::from("ISO-10303-21;\nHEADER;\n");
    out.push_str(&step_header());
    out.push_str("ENDSEC;\nDATA;\n");
    for inst in &doc.instances {
        out.push_str(&format!("#{} = {});\n", inst.id, serialize_values(&inst.entity, &inst.attributes)));
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(ConversionResult {
        output: out.into_bytes(),
        output_format: CadFormat::Step,
        converted_entities: doc.instances.len(),
        skipped: Vec::new(),
    })
}

/// Serialize `ENTITY(values)` with STEP literals: strings re-escaped, reals
/// guaranteed a decimal point, enums dotted, references as `#n`.
fn serialize_values(entity: &str, attrs: &[Value]) -> String {
    let mut out = format!("{entity}(");
    for (i, attr) in attrs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        serialize_value(attr, &mut out);
    }
    out.push(')');
    out
}

fn serialize_value(value: &Value, out: &mut String) {
    match value {
        Value::Unset => out.push('$'),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Real(r) => out.push_str(&fmt_step_real(*r)),
        Value::Str(s) => {
            out.push('\'');
            for ch in s.chars() {
                if ch == '\'' {
                    out.push_str("''");
                } else {
                    out.push(ch);
                }
            }
            out.push('\'');
        }
        Value::Ref(id) => out.push_str(&format!("#{id}")),
        Value::Enum(e) => out.push_str(&format!(".{e}.")),
        Value::List(items) => {
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                serialize_value(item, out);
            }
            out.push(')');
        }
        Value::Entity(name, attrs) => {
            let inner = serialize_values(name, attrs);
            // serialize_values appends the closing paren itself.
            out.push_str(&inner);
        }
    }
}

fn normalize_iges(input: &[u8]) -> Result<ConversionResult, CadError> {
    let file = iges::parse_iges(input);
    let converted = file.entities.len();
    let output = iges::write_iges(&file);
    Ok(ConversionResult {
        output,
        output_format: CadFormat::Iges,
        converted_entities: converted,
        skipped: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// STEP → IGES
// ---------------------------------------------------------------------------

fn step_to_iges(input: &[u8]) -> Result<ConversionResult, CadError> {
    let doc = parse_ifc(input).map_err(|_| CadError::Invalid("unparseable STEP file"))?;

    let mut entities: Vec<IgesEntity> = Vec::new();
    let mut skipped: Vec<(String, usize)> = Vec::new();
    let mut skip = |name: &str| {
        let name = name.to_ascii_uppercase();
        match skipped.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => skipped.push((name, 1)),
        }
    };

    for inst in &doc.instances {
        let name = inst.entity.as_str();
        if name.eq_ignore_ascii_case("CARTESIAN_POINT") {
            if let Some(coords) = point_coords(&inst.attributes) {
                entities.push(IgesEntity {
                    de_number: 0,
                    de_type: 116,
                    fields: blank_fields("116"),
                    param_pointer: 0,
                    params: vec![
                        IgesParam::Real(coords[0]),
                        IgesParam::Real(coords[1]),
                        IgesParam::Real(coords[2]),
                        IgesParam::Pointer(0),
                    ],
                });
                continue;
            }
            skip(name);
        } else if name.eq_ignore_ascii_case("LINE") {
            match line_params(&doc, &inst.attributes) {
                Some((p1, p2)) => entities.push(IgesEntity {
                    de_number: 0,
                    de_type: 110,
                    fields: blank_fields("110"),
                    param_pointer: 0,
                    params: vec![
                        IgesParam::Real(p1[0]),
                        IgesParam::Real(p1[1]),
                        IgesParam::Real(p1[2]),
                        IgesParam::Real(p2[0]),
                        IgesParam::Real(p2[1]),
                        IgesParam::Real(p2[2]),
                    ],
                }),
                None => skip(name),
            }
        } else if name.eq_ignore_ascii_case("CIRCLE") {
            match step_circle_to_arc(&doc, &inst.attributes) {
                Some(params) => entities.push(IgesEntity {
                    de_number: 0,
                    de_type: 100,
                    fields: blank_fields("100"),
                    param_pointer: 0,
                    params,
                }),
                None => skip(name),
            }
        } else {
            skip(name);
        }
    }

    let file = IgesFile {
        start: vec![format!(
            "Converted from STEP by TPT CAD Format ({} entities)",
            entities.len()
        )],
        globals: Vec::new(), // writer emits canonical defaults
        entities,
    };
    let converted = file.entities.len();
    let output = iges::write_iges(&file);
    Ok(ConversionResult {
        output,
        output_format: CadFormat::Iges,
        converted_entities: converted,
        skipped: summarize_skipped(skipped),
    })
}

fn blank_fields(type_number: &str) -> [[String; 9]; 2] {
    let mut fields: [[String; 9]; 2] = std::array::from_fn(|_| std::array::from_fn(|_| "0".to_string()));
    fields[0][0] = type_number.to_string();
    fields[1][0] = type_number.to_string();
    fields
}

/// CARTESIAN_POINT attrs `(name, coords)` → `[x, y, z]` (padded, finite).
fn point_coords(attrs: &[Value]) -> Option<[f64; 3]> {
    let Value::List(coords) = attrs.get(1)? else {
        return None;
    };
    let mut out = [0.0; 3];
    for (i, v) in coords.iter().take(3).enumerate() {
        let r = match v {
            Value::Real(r) => *r,
            Value::Int(i) => *i as f64,
            _ => return None,
        };
        if !r.is_finite() {
            return None;
        }
        out[i] = r;
    }
    Some(out)
}

/// Resolve a `Ref` to a CARTESIAN_POINT's `[x, y, z]`.
fn resolve_point(doc: &tpt_io_ifc::IfcDocument, attr: &Value) -> Option<[f64; 3]> {
    let Value::Ref(id) = attr else { return None };
    let target = doc.by_id(*id)?;
    point_coords(&target.attributes)
}

/// LINE attrs `(name, pnt, dir)` → both endpoints, using the VECTOR's
/// orientation and magnitude: `p2 = p1 + normalize(dir) * magnitude`.
fn line_params(doc: &tpt_io_ifc::IfcDocument, attrs: &[Value]) -> Option<([f64; 3], [f64; 3])> {
    let p1 = resolve_point(doc, attrs.get(1)?)?;
    let Value::Ref(dir_id) = attrs.get(2)? else { return None };
    let vector = doc.by_id(*dir_id)?;
    if !vector.entity.eq_ignore_ascii_case("VECTOR") {
        return None;
    }
    let Value::Ref(orientation_id) = vector.attributes.get(1)? else {
        return None;
    };
    let orientation = doc.by_id(*orientation_id)?;
    let Value::List(ratios) = orientation.attributes.get(1)? else {
        return None;
    };
    let mut dir = [0.0f64; 3];
    for (i, v) in ratios.iter().take(3).enumerate() {
        dir[i] = match v {
            Value::Real(r) => *r,
            Value::Int(i) => *i as f64,
            _ => return None,
        };
    }
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len == 0.0 || !len.is_finite() {
        return None;
    }
    let Value::Real(magnitude) = vector.attributes.get(2)? else {
        return None;
    };
    if !magnitude.is_finite() || *magnitude < 0.0 {
        return None;
    }
    let p2 = [
        p1[0] + dir[0] / len * magnitude,
        p1[1] + dir[1] / len * magnitude,
        p1[2] + dir[2] / len * magnitude,
    ];
    Some((p1, p2))
}

/// CIRCLE attrs `(name, position, radius)` → IGES 100 params, only when the
/// placement axis is the model Z (otherwise a 124 transform is required).
fn step_circle_to_arc(doc: &tpt_io_ifc::IfcDocument, attrs: &[Value]) -> Option<Vec<IgesParam>> {
    let Value::Ref(pos_id) = attrs.get(1)? else { return None };
    let position = doc.by_id(*pos_id)?;
    if !position.entity.eq_ignore_ascii_case("AXIS2_PLACEMENT_3D") {
        return None;
    }
    let Value::Real(radius) = attrs.get(2)? else { return None };
    if !radius.is_finite() || *radius <= 0.0 {
        return None;
    }
    let center = resolve_point(doc, position.attributes.get(1)?)?;
    let axis = resolve_direction(doc, position.attributes.get(2)).unwrap_or([0.0, 0.0, 1.0]);
    if (axis[2].abs() - 1.0).abs() > 1e-9 || (axis[0].abs() > 1e-9) || (axis[1].abs() > 1e-9) {
        return None; // tilted plane — needs a transformation-matrix chain
    }
    let ref_dir = resolve_direction(doc, position.attributes.get(3)).unwrap_or([1.0, 0.0, 0.0]);
    let start = [center[0] + ref_dir[0] * radius, center[1] + ref_dir[1] * radius];
    Some(vec![
        IgesParam::Real(center[2]),
        IgesParam::Real(center[0]),
        IgesParam::Real(center[1]),
        IgesParam::Real(start[0]),
        IgesParam::Real(start[1]),
        // Full circle: end point equals start point.
        IgesParam::Real(start[0]),
        IgesParam::Real(start[1]),
    ])
}

fn resolve_direction(doc: &tpt_io_ifc::IfcDocument, attr: Option<&Value>) -> Option<[f64; 3]> {
    let Value::Ref(id) = attr? else { return None };
    let direction = doc.by_id(*id)?;
    let Value::List(ratios) = direction.attributes.get(1)? else {
        return None;
    };
    let mut out = [0.0f64; 3];
    for (i, v) in ratios.iter().take(3).enumerate() {
        out[i] = match v {
            Value::Real(r) => *r,
            Value::Int(i) => *i as f64,
            _ => return None,
        };
    }
    let len = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
    if len == 0.0 {
        return None;
    }
    Some([out[0] / len, out[1] / len, out[2] / len])
}

// ---------------------------------------------------------------------------
// IGES → STEP
// ---------------------------------------------------------------------------

fn iges_to_step(input: &[u8]) -> Result<ConversionResult, CadError> {
    let file = iges::parse_iges(input);

    let mut instances: Vec<(u64, String)> = Vec::new(); // (id, serialized)
    let mut skipped: Vec<(String, usize)> = Vec::new();
    let mut next_id = 1u64;
    let alloc = |next_id: &mut u64| {
        let id = *next_id;
        *next_id += 1;
        id
    };

    for entity in &file.entities {
        match entity.de_type {
            116 => {
                // Point: X, Y, Z, pointer.
                if let (Some(x), Some(y), Some(z)) = (
                    param_real(&entity.params, 0),
                    param_real(&entity.params, 1),
                    param_real(&entity.params, 2),
                ) {
                    let id = alloc(&mut next_id);
                    instances.push((
                        id,
                        format!("CARTESIAN_POINT('',({},{},{}))", fmt_step_real(x), fmt_step_real(y), fmt_step_real(z)),
                    ));
                } else {
                    bump_skip(&mut skipped, "Point (116)");
                }
            }
            110 => {
                // Line: X1,Y1,Z1,X2,Y2,Z2.
                let (Some(x1), Some(y1), Some(z1), Some(x2), Some(y2), Some(z2)) = (
                    param_real(&entity.params, 0),
                    param_real(&entity.params, 1),
                    param_real(&entity.params, 2),
                    param_real(&entity.params, 3),
                    param_real(&entity.params, 4),
                    param_real(&entity.params, 5),
                ) else {
                    bump_skip(&mut skipped, "Line (110)");
                    continue;
                };
                let dx = x2 - x1;
                let dy = y2 - y1;
                let dz = z2 - z1;
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len == 0.0 || !len.is_finite() {
                    bump_skip(&mut skipped, "Line (110, zero length)");
                    continue;
                }
                let p1 = alloc(&mut next_id);
                instances.push((
                    p1,
                    format!(
                        "CARTESIAN_POINT('',({},{},{}))",
                        fmt_step_real(x1), fmt_step_real(y1), fmt_step_real(z1)
                    ),
                ));
                let axis = alloc(&mut next_id);
                instances.push((
                    axis,
                    format!(
                        "DIRECTION('',({},{},{}))",
                        fmt_step_real(dx / len), fmt_step_real(dy / len), fmt_step_real(dz / len)
                    ),
                ));
                let vec_id = alloc(&mut next_id);
                instances.push((
                    vec_id,
                    format!("VECTOR('',#{axis},{})", fmt_step_real(len)),
                ));
                let line = alloc(&mut next_id);
                instances.push((line, format!("LINE('',#{p1},#{vec_id})")));
            }
            100 => {
                // Circular arc: ZT, center X/Y, start X/Y, end X/Y (2D, Z plane).
                if entity.params.len() < 7 {
                    bump_skip(&mut skipped, "Circular Arc (100)");
                    continue;
                }
                let (Some(zt), Some(cx), Some(cy), Some(x1), Some(y1)) = (
                    param_real(&entity.params, 0),
                    param_real(&entity.params, 1),
                    param_real(&entity.params, 2),
                    param_real(&entity.params, 3),
                    param_real(&entity.params, 4),
                ) else {
                    bump_skip(&mut skipped, "Circular Arc (100)");
                    continue;
                };
                let radius = ((x1 - cx).powi(2) + (y1 - cy).powi(2)).sqrt();
                if radius == 0.0 || !radius.is_finite() {
                    bump_skip(&mut skipped, "Circular Arc (100, zero radius)");
                    continue;
                }
                let center_pt = alloc(&mut next_id);
                instances.push((
                    center_pt,
                    format!(
                        "CARTESIAN_POINT('',({},{},{}))",
                        fmt_step_real(cx), fmt_step_real(cy), fmt_step_real(zt)
                    ),
                ));
                let z_dir = alloc(&mut next_id);
                instances.push((z_dir, "DIRECTION('',(0.,0.,1.))".to_string()));
                // Arcs carry orientation via start→end; a full STEP CIRCLE
                // (untrimmed) has no sweep, so the ref direction points at
                // the arc's start point.
                let ux = (x1 - cx) / radius;
                let uy = (y1 - cy) / radius;
                let x_dir = alloc(&mut next_id);
                instances.push((
                    x_dir,
                    format!("DIRECTION('',({},{},0.))", fmt_step_real(ux), fmt_step_real(uy)),
                ));
                let placement = alloc(&mut next_id);
                instances.push((
                    placement,
                    format!("AXIS2_PLACEMENT_3D('',#{center_pt},#{z_dir},#{x_dir})"),
                ));
                let circle = alloc(&mut next_id);
                instances.push((
                    circle,
                    format!("CIRCLE('',#{placement},{})", fmt_step_real(radius)),
                ));
            }
            other => bump_skip(&mut skipped, &format!("Type {other}")),
        }
    }

    let mut out = String::from("ISO-10303-21;\nHEADER;\n");
    out.push_str(&step_header());
    out.push_str("ENDSEC;\nDATA;\n");
    for (id, serialized) in &instances {
        out.push_str(&format!("#{id} = {serialized};\n"));
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");

    Ok(ConversionResult {
        output: out.into_bytes(),
        output_format: CadFormat::Step,
        converted_entities: instances.len(),
        skipped: summarize_skipped(skipped),
    })
}

fn param_real(params: &[IgesParam], index: usize) -> Option<f64> {
    match params.get(index)? {
        IgesParam::Real(r) => Some(*r),
        IgesParam::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn bump_skip(skipped: &mut Vec<(String, usize)>, name: &str) {
    match skipped.iter_mut().find(|(n, _)| n == name) {
        Some((_, count)) => *count += 1,
        None => skipped.push((name.to_string(), 1)),
    }
}

fn summarize_skipped(mut skipped: Vec<(String, usize)>) -> Vec<String> {
    skipped.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    skipped.into_iter().map(|(name, count)| format!("{name} ×{count}")).collect()
}

/// Standard canonical STEP header for converted files.
fn step_header() -> String {
    "FILE_DESCRIPTION(('Converted by TPT CAD Format'),'2;1');\n\
     FILE_NAME('converted','1970-01-01T00:00:00',('TPT CAD Format'),\
     ('TPT Solutions'),'TPT CAD Format','','');\n\
     FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\n"
        .to_string()
}

/// STEP reals always carry a decimal point. Non-finite values are clamped to
/// huge finite neighbours rather than `NaN`/`inf` (which no CAD system
/// parses); callers pre-filter degenerates, so this is a last resort.
fn fmt_step_real(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "0.".to_string()
        } else if value > 0.0 {
            "1.E308".to_string()
        } else {
            "-1.E308".to_string()
        };
    }
    // Very small/large magnitudes use STEP exponential form ("1.5E-9");
    // everything else uses plain decimal with trailing zeros trimmed.
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-4..1e12).contains(&magnitude) {
        return format!("{value:.6E}"); // "1.500000E-9" — already dotted
    }
    let formatted = format!("{value:.9}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed.contains('.') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

/// Convenience: convert and produce a mini-report for the desktop shell to
/// append to its conversion log.
pub fn conversion_report(source: &str, result: &ConversionResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "TPT CAD Format — conversion report for {source}\n"
    ));
    out.push_str(&format!(
        "Target format: {}\nEntities converted/normalized: {}\n",
        result.output_format.label(),
        result.converted_entities
    ));
    if result.skipped.is_empty() {
        out.push_str("Skipped: none — everything mapped.\n");
    } else {
        out.push_str("Skipped entity types (not silently dropped — listed here):\n");
        for row in &result.skipped {
            out.push_str(&format!("  {row}\n"));
        }
    }
    out
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iges::parse_iges;

    const STEP_IN: &str = "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('X'));\nENDSEC;\nDATA;\n\
        #1 = CARTESIAN_POINT('',(1.,2.,3.));\n\
        #2 = DIRECTION('',(1.,0.,0.));\n\
        #3 = VECTOR('',#2,4.);\n\
        #4 = LINE('',#1,#3);\n\
        #5 = AXIS2_PLACEMENT_3D('',#1,#6,#2);\n\
        #6 = DIRECTION('',(0.,0.,1.));\n\
        #7 = CIRCLE('',#5,2.5);\n\
        ENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn step_to_iges_maps_point_line_circle() {
        let result = convert(STEP_IN.as_bytes(), CadFormat::Iges).unwrap();
        assert_eq!(result.output_format, CadFormat::Iges);
        assert_eq!(result.converted_entities, 3);
        let file = parse_iges(&result.output);
        let types: Vec<u64> = file.entities.iter().map(|e| e.de_type).collect();
        assert_eq!(types, vec![116, 110, 100]);
        // Line endpoint: p1 + dir*magnitude = (5,2,3).
        let line = &file.entities[1];
        assert_eq!(param_real(&line.params, 3), Some(5.0));
        // Skips: DIRECTION ×2, VECTOR, AXIS2_PLACEMENT_3D.
        assert_eq!(result.skipped.len(), 3, "{:?}", result.skipped);
    }

    #[test]
    fn iges_to_step_roundtrips_point() {
        let step_in = "ISO-10303-21;\nDATA;\n#1 = CARTESIAN_POINT('',(7.,8.,9.));\n\
            ENDSEC;\nEND-ISO-10303-21;\n";
        let to_iges = convert(step_in.as_bytes(), CadFormat::Iges).unwrap();
        let back = convert(&to_iges.output, CadFormat::Step).unwrap();
        assert_eq!(back.output_format, CadFormat::Step);
        assert_eq!(back.converted_entities, 1);
        let text = String::from_utf8(back.output).unwrap();
        assert!(text.contains("CARTESIAN_POINT('',(7.,8.,9.))"), "{text}");
    }

    #[test]
    fn normalization_rejects_junk() {
        assert!(matches!(
            convert(b"junk", CadFormat::Step),
            Err(CadError::UnsupportedFormat)
        ));
    }
}
