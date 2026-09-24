//! Canonical fixed-column re-emission of an [`IgesFile`].
//!
//! The writer recomputes everything positional — sequence numbers, the
//! D-entry parameter pointers, P-record wrapping at column 72, and the
//! Terminator counts — so `parse(write(parse(x)))` is structurally equal to
//! `parse(x)` for well-formed files. Entity label fields are preserved as
//! written; numeric fields are re-right-justified; parameter strings are
//! re-emitted with the file's delimiters (Global fields 1 and 2, defaulting
//! to `,` and `;`).

use std::fmt::Write as _;

use super::parse::hollerith_char;
use super::{IgesFile, IgesParam};

/// Serialize back to canonical fixed-column IGES (80-column records, `\n`
/// line endings). Satisfies: `parse(write(parse(x))) == parse(x)`
/// structurally, for well-formed fixture files.
pub fn write_iges(file: &IgesFile) -> Vec<u8> {
    let pdelim = delim_at(&file.globals, 0, ',');
    let rdelim = delim_at(&file.globals, 1, ';');
    let sep = pdelim.to_string();

    // Parameter data first: the D entries quote the P record each entity
    // starts at, and the P record count feeds the Terminator.
    let p_chunks: Vec<Vec<String>> = file
        .entities
        .iter()
        .map(|e| {
            let mut body = String::new();
            for (k, p) in e.params.iter().enumerate() {
                if k > 0 {
                    body.push(pdelim);
                }
                body.push_str(&emit_param(p));
            }
            body.push(rdelim);
            chunk_chars(&body, 56) // columns 17-72 hold 56 characters
        })
        .collect();
    let mut entity_p_start = Vec::with_capacity(file.entities.len());
    let mut running = 1u64;
    for chunks in &p_chunks {
        entity_p_start.push(running);
        running += chunks.len() as u64;
    }
    let p_count = running - 1;

    let globals_joined = if file.globals.is_empty() {
        String::new()
    } else {
        format!("{}{rdelim}", file.globals.join(&sep))
    };
    let g_chunks = chunk_chars(&globals_joined, 72);

    let mut out = String::new();
    // Sequence numbers restart at 1 in every section.
    let mut s_seq = 0u64;
    for line in &file.start {
        s_seq += 1;
        push_record(&mut out, line, 'S', s_seq);
    }
    let mut g_seq = 0u64;
    for chunk in &g_chunks {
        g_seq += 1;
        push_record(&mut out, chunk, 'G', g_seq);
    }
    let mut d_seq = 0u64;
    for (i, e) in file.entities.iter().enumerate() {
        let mut line1 = String::new();
        for (j, raw) in e.fields[0].iter().enumerate() {
            // Field 2 of D line 1 is the parameter data pointer; recompute it
            // so the emitted file is self-consistent.
            let value = if j == 1 { entity_p_start[i].to_string() } else { raw.clone() };
            line1.push_str(&fit_field(&value));
        }
        d_seq += 1;
        push_record(&mut out, &line1, 'D', d_seq);
        let mut line2 = String::new();
        for raw in &e.fields[1] {
            line2.push_str(&fit_field(raw));
        }
        d_seq += 1;
        push_record(&mut out, &line2, 'D', d_seq);
    }
    let mut pseq = 0u64;
    for (i, chunks) in p_chunks.iter().enumerate() {
        // Every P record repeats the entity type and the DE pointer (the
        // entity's first D record, 2i-1).
        let de = 2 * i as u64 + 1;
        let head = format!(
            "{}{}",
            fit_field(&file.entities[i].de_type.to_string()),
            fit_field(&de.to_string())
        );
        for chunk in chunks {
            pseq += 1;
            let mut content = head.clone();
            content.push_str(chunk);
            push_record(&mut out, &content, 'P', pseq);
        }
    }
    let t_content = format!(
        "{}{}{}{}{}",
        fit_field(&file.start.len().to_string()),
        fit_field(&g_chunks.len().to_string()),
        fit_field(&(2 * file.entities.len()).to_string()),
        fit_field(&p_count.to_string()),
        fit_field("1"),
    );
    push_record(&mut out, &t_content, 'T', 1);
    out.into_bytes()
}

/// Resolve a delimiter Global field (a single-character Hollerith constant)
/// or fall back to the default.
fn delim_at(globals: &[String], idx: usize, default: char) -> char {
    globals.get(idx).and_then(|t| hollerith_char(t)).unwrap_or(default)
}

/// One parameter back to text. `Int` and `Pointer` emit identically; reals
/// keep a decimal point or exponent so they re-parse as reals; strings are
/// re-encoded as Hollerith constants (which safely contain delimiters).
fn emit_param(p: &IgesParam) -> String {
    match p {
        IgesParam::Int(v) | IgesParam::Pointer(v) => v.to_string(),
        IgesParam::Real(v) => format!("{v:?}"),
        IgesParam::Str(s) => format!("{}H{s}", s.chars().count()),
        IgesParam::Blank => String::new(),
    }
}

/// Fit a raw directory field into its 8 columns: numeric text is
/// right-justified, everything else left-justified; longer values truncate.
fn fit_field(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "        ".to_string();
    }
    let numeric = t.bytes().all(|b| b.is_ascii_digit());
    let padded = if numeric { format!("{t:>8}") } else { format!("{t:<8}") };
    let chars: Vec<char> = padded.chars().collect();
    if chars.len() <= 8 {
        padded
    } else if numeric {
        chars[chars.len() - 8..].iter().collect()
    } else {
        chars[..8].iter().collect()
    }
}

/// Pad/truncate free-format content to exactly `width` columns.
fn fit_content(content: &str, width: usize) -> String {
    let mut c: String = content.chars().take(width).collect();
    let used = c.chars().count();
    c.extend(std::iter::repeat_n(' ', width - used));
    c
}

fn push_record(out: &mut String, content: &str, letter: char, seq: u64) {
    let _ = writeln!(out, "{}{letter}{seq:>7}", fit_content(content, 72));
}

/// Split into chunks of at most `width` characters; the parser concatenates
/// them back, so any split point round-trips.
fn chunk_chars(s: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    chars.chunks(width).map(|c| c.iter().collect()).collect()
}
