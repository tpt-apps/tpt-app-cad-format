//! Record framing and the structural parse into [`IgesFile`].
//!
//! Bytes are split into records on `\n` (tolerating `\r\n` and a missing
//! final newline). Every well-framed record carries a section letter in
//! column 73 and a right-justified sequence number in columns 74-80; the
//! framer groups the records into S/G/D/P/T sections, enforces section
//! order and per-section sequence contiguity, and the D/P parsers then
//! build the entity list the Pro converter consumes.

use super::{display_name, Sink};
use crate::model::{Finding, FindingKind, Severity};

// ---------- public parsed model ----------

/// One parsed IGES file (structural view).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IgesFile {
    /// S-section lines (free text from columns 1-72).
    pub start: Vec<String>,
    /// G-section raw fields (delimiter-aware split). The delimiter fields
    /// keep their written Hollerith form, e.g. `1H,`.
    pub globals: Vec<String>,
    /// D entries plus P parameters, in directory-entry order.
    pub entities: Vec<IgesEntity>,
}

/// One directory entry (two D records) with its parameter data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IgesEntity {
    /// 1-based entity index (not the D-record sequence number).
    pub de_number: u64,
    /// Entity type number from the first D record (0 when unreadable).
    pub de_type: u64,
    /// P-section sequence number where this entity's parameters start.
    pub param_pointer: u64,
    /// Raw D-record fields, trimmed, as written: line 1 first, then line 2.
    pub fields: [[String; 9]; 2],
    /// Parsed parameter data (the free-format fields of columns 17-72).
    pub params: Vec<IgesParam>,
}

/// One IGES parameter value.
///
/// Pointers are integers whose meaning depends on the referencing field;
/// [`IgesParam::Int`] and [`IgesParam::Pointer`] are emitted identically —
/// the distinction is documentation for the conversion layer.
#[derive(Debug, Clone, PartialEq)]
pub enum IgesParam {
    Int(i64),
    Real(f64),
    Str(String),
    Pointer(i64),
    Blank,
}

/// Parse raw IGES bytes into the structural [`IgesFile`] view.
///
/// Never panics and always returns a file skeleton: records that cannot be
/// framed are skipped (run `validate_iges` to surface them as findings).
pub fn parse_iges(bytes: &[u8]) -> IgesFile {
    parse_with_layout(bytes).0
}

// ---------- internal layout view ----------

/// One framed P record: headers parsed, body kept as text.
pub(crate) struct PRecord {
    pub line: usize,
    pub seq: u64,
    pub etype: Option<u64>,
    pub etype_raw: String,
    pub de: Option<u64>,
    pub de_raw: String,
    pub body: String,
}

/// How much Parameter Data actually belongs to one entity.
pub(crate) struct EntitySpan {
    /// The D record's parameter pointer parsed as a number at all.
    pub pointer_ok: bool,
    /// The D record's declared Parameter Line Count parsed as a number at all.
    pub declared_ok: bool,
    pub declared: u64,
    /// Indices into [`Layout::param_records`] spanned by this entity.
    pub records: Vec<usize>,
    /// File line of the first spanned P record, when any.
    pub first_line: Option<usize>,
}

/// Section layout facts the semantic checks need beyond [`IgesFile`].
pub(crate) struct Layout {
    /// Actual record counts per section, in S G D P T order.
    pub actual: [u64; 5],
    /// File line of every D record, in section order.
    pub dir_lines: Vec<usize>,
    pub param_records: Vec<PRecord>,
    pub term: Option<TermRecord>,
    /// File line of the first G record, when the section exists.
    pub global_line: Option<usize>,
    pub param_delim: char,
    pub record_delim: char,
    /// One span per entity, parallel to [`IgesFile::entities`].
    pub spans: Vec<EntitySpan>,
}

pub(crate) struct TermRecord {
    pub line: usize,
    /// Raw count fields: S, G, D (line count), P, T.
    pub counts: [String; 5],
}

impl Layout {
    fn empty() -> Self {
        Layout {
            actual: [0; 5],
            dir_lines: Vec::new(),
            param_records: Vec::new(),
            term: None,
            global_line: None,
            param_delim: ',',
            record_delim: ';',
            spans: Vec::new(),
        }
    }
}

struct RawRecord {
    line: usize,
    text: String,
}

struct Framed {
    start: Vec<String>,
    global: Vec<String>,
    dir: Vec<String>,
    param: Vec<String>,
    term: Vec<String>,
    dir_lines: Vec<usize>,
    param_lines: Vec<usize>,
    global_line: Option<usize>,
    term_line: Option<usize>,
    counts: [u64; 5],
    valid: usize,
}

pub(crate) fn parse_with_layout(bytes: &[u8]) -> (IgesFile, Layout, Vec<Finding>, bool) {
    let text = String::from_utf8_lossy(bytes);
    let records: Vec<RawRecord> = text
        .lines()
        .enumerate()
        .map(|(i, l)| RawRecord { line: i + 1, text: l.trim_end_matches('\r').to_string() })
        .collect();
    let mut findings = Vec::new();
    let mut file = IgesFile::default();
    let mut layout = Layout::empty();

    if records.is_empty() {
        findings.push(
            Finding::new(
                FindingKind::MissingSection,
                Severity::Error,
                "no IGES records found — the file is empty",
            )
            .at(1),
        );
        return (file, layout, findings, false);
    }

    let framed = {
        let mut sink = Sink::new(&mut findings);
        frame(&records, &mut sink)
    };

    if framed.valid == 0 {
        // Not one record carried the IGES section layout — do not flood the
        // report with per-record framing noise, say so once instead.
        findings.clear();
        findings.push(
            Finding::new(
                FindingKind::NotParseable,
                Severity::Error,
                "no IGES 80-column section records found — the bytes do not carry the IGES \
                 record layout",
            )
            .at(1),
        );
        return (file, layout, findings, false);
    }

    layout.actual = framed.counts;
    layout.dir_lines = framed.dir_lines.clone();
    layout.global_line = framed.global_line;
    file.start = framed.start.iter().map(|c| c.trim_end().to_string()).collect();

    {
        let mut sink = Sink::new(&mut findings);
        let (globals, pdelim, rdelim) =
            parse_globals(&framed.global, framed.global_line, &mut sink);
        file.globals = globals;
        layout.param_delim = pdelim;
        layout.record_delim = rdelim;
    }

    for (idx, rec) in framed.param.iter().enumerate() {
        let chars: Vec<char> = rec.chars().collect();
        let field = |a: usize, b: usize| -> String {
            chars[a..b].iter().collect::<String>().trim().to_string()
        };
        let etype_raw = field(0, 8);
        let de_raw = field(8, 16);
        let body = chars[16..72].iter().collect::<String>();
        layout.param_records.push(PRecord {
            line: framed.param_lines[idx],
            seq: idx as u64 + 1,
            etype: etype_raw.parse::<u64>().ok(),
            etype_raw: etype_raw.clone(),
            de: de_raw.parse::<u64>().ok(),
            de_raw,
            body: body.trim_end().to_string(),
        });
    }

    if let Some(rec) = framed.term.first() {
        let chars: Vec<char> = rec.chars().collect();
        let field = |a: usize, b: usize| -> String {
            chars[a..b].iter().collect::<String>().trim().to_string()
        };
        layout.term = Some(TermRecord {
            line: framed.term_line.unwrap_or(1),
            counts: [field(0, 8), field(8, 16), field(16, 24), field(24, 32), field(32, 40)],
        });
    }

    {
        let mut sink = Sink::new(&mut findings);
        build_entities(&framed, &mut layout, &mut file, &mut sink);
    }

    (file, layout, findings, true)
}

/// Group records into sections, enforcing the section order (S, G, D, P, T),
/// the single-record Terminator, and per-section sequence contiguity.
fn frame(records: &[RawRecord], sink: &mut Sink<'_>) -> Framed {
    let mut f = Framed {
        start: Vec::new(),
        global: Vec::new(),
        dir: Vec::new(),
        param: Vec::new(),
        term: Vec::new(),
        dir_lines: Vec::new(),
        param_lines: Vec::new(),
        global_line: None,
        term_line: None,
        counts: [0; 5],
        valid: 0,
    };
    let mut current_rank = 0usize;
    let mut since_bad = false;
    for rec in records {
        let chars: Vec<char> = rec.text.chars().collect();
        if chars.len() != 80 {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "record {} is {} column(s), expected 80 — broken record framing",
                        rec.line,
                        chars.len()
                    ),
                )
                .at(rec.line),
            );
            since_bad = true;
            continue;
        }
        let letter = chars[72];
        let rank = match letter {
            'S' => Some(0usize),
            'G' => Some(1),
            'D' => Some(2),
            'P' => Some(3),
            'T' => Some(4),
            _ => None,
        };
        let seq_text: String = chars[73..80].iter().collect();
        let seq = seq_text.trim().parse::<u64>().ok();
        let (Some(rank), Some(seq)) = (rank, seq) else {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "record {} has an unknown section letter '{}' or an unreadable sequence \
                         number — expected S/G/D/P/T in column 73 and a number in columns 74-80",
                        rec.line, letter
                    ),
                )
                .at(rec.line),
            );
            since_bad = true;
            continue;
        };
        f.valid += 1;
        if rank < current_rank {
            sink.push(
                Finding::new(
                    FindingKind::SequenceGap,
                    Severity::Error,
                    format!(
                        "section {letter} record appears after section {} began — sections must \
                         run in the order S, G, D, P, T",
                        rank_letter(current_rank)
                    ),
                )
                .at(rec.line),
            );
            since_bad = true;
            continue;
        }
        if letter == 'T' && f.counts[4] == 1 {
            sink.push(
                Finding::new(
                    FindingKind::SequenceGap,
                    Severity::Error,
                    "terminator section must hold exactly one record".to_string(),
                )
                .at(rec.line),
            );
            continue;
        }
        current_rank = rank;
        let expected = f.counts[rank] + 1;
        if since_bad {
            // A broken record intervened: re-sync to whatever this section
            // now shows instead of double-reporting the gap.
            since_bad = false;
        } else if seq != expected {
            sink.push(
                Finding::new(
                    FindingKind::SequenceGap,
                    Severity::Error,
                    format!("{letter} section sequence number is {seq}, expected {expected}"),
                )
                .at(rec.line),
            );
        }
        f.counts[rank] = seq;
        match letter {
            'S' => f.start.push(chars[..72].iter().collect()),
            'G' => {
                if f.global_line.is_none() {
                    f.global_line = Some(rec.line);
                }
                f.global.push(chars[..72].iter().collect());
            }
            'D' => {
                f.dir.push(chars.iter().collect());
                f.dir_lines.push(rec.line);
            }
            'P' => {
                f.param.push(chars.iter().collect());
                f.param_lines.push(rec.line);
            }
            'T' => {
                if f.term_line.is_none() {
                    f.term_line = Some(rec.line);
                }
                f.term.push(chars.iter().collect());
            }
            _ => {}
        }
    }
    f
}

fn rank_letter(rank: usize) -> char {
    match rank {
        0 => 'S',
        1 => 'G',
        2 => 'D',
        3 => 'P',
        _ => 'T',
    }
}

/// Split the Global section into raw fields, resolving the parameter and
/// record delimiters (Global fields 1 and 2, Hollerith when redefined).
fn parse_globals(
    records: &[String],
    line: Option<usize>,
    sink: &mut Sink<'_>,
) -> (Vec<String>, char, char) {
    if records.is_empty() {
        sink.push(
            Finding::new(
                FindingKind::MissingSection,
                Severity::Warning,
                "no Global section — delimiters and units fall back to the IGES defaults",
            )
            .at(line.unwrap_or(1)),
        );
        return (Vec::new(), ',', ';');
    }
    let at = line.unwrap_or(1);
    let concat: String = records.iter().map(|c| c.trim_end()).collect();
    let chars: Vec<char> = concat.chars().collect();
    let mut globals: Vec<String> = Vec::new();
    let (mut pdelim, mut rdelim) = (',', ';');
    let mut pos = 0usize;

    // Field 1: the parameter delimiter (a Hollerith constant, `1H,` for the
    // default). Field 2: the record delimiter, same rules.
    if let Some((end, _)) = hollerith_at(&chars, pos) {
        let token: String = chars[pos..end].iter().collect();
        match hollerith_char(&token) {
            Some(c) => pdelim = c,
            None => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    "Global parameter delimiter must be a single character — default ',' assumed"
                        .to_string(),
                )
                .at(at),
            ),
        }
        globals.push(token);
        pos = end;
        if pos < chars.len() && chars[pos] == ',' {
            pos += 1;
        }
        if let Some((end2, _)) = hollerith_at(&chars, pos) {
            let token2: String = chars[pos..end2].iter().collect();
            match hollerith_char(&token2) {
                Some(c) => rdelim = c,
                None => sink.push(
                    Finding::new(
                        FindingKind::MalformedEntity,
                        Severity::Warning,
                        "Global record delimiter must be a single character — default ';' assumed"
                            .to_string(),
                    )
                    .at(at),
                ),
            }
            globals.push(token2);
            pos = end2;
            if pos < chars.len() && chars[pos] == ',' {
                pos += 1;
            }
        }
    } else if !chars.is_empty() && chars[0] == ',' {
        // Field 1 written as a bare comma: defaults apply, and the comma
        // only separates fields. Keep it verbatim so writing round-trips.
        globals.push(",".to_string());
        pos = 1;
    }

    let rest: String = chars[pos.min(chars.len())..].iter().collect();
    let (mut fields, _, _) = split_fields(&rest, pdelim, rdelim);
    while fields.last().is_some_and(|f| f.is_empty()) {
        fields.pop();
    }
    globals.extend(fields);
    (globals, pdelim, rdelim)
}

/// Split on `pdelim`, honouring Hollerith constants (`nHtext`, which swallow
/// both delimiter kinds). Stops at `rdelim`; returns whether it was reached
/// and the text after it.
fn split_fields(s: &str, pdelim: char, rdelim: char) -> (Vec<String>, bool, String) {
    let chars: Vec<char> = s.chars().collect();
    let mut fields: Vec<String> = Vec::new();
    let mut i = 0usize;
    let terminated;
    loop {
        if let Some((end, _)) = hollerith_at(&chars, i) {
            fields.push(chars[i..end].iter().collect());
            i = end;
            if i >= chars.len() {
                terminated = false;
                break;
            }
            if chars[i] == rdelim {
                i += 1;
                terminated = true;
                break;
            }
            if chars[i] == pdelim {
                i += 1;
            }
            // Anything else is junk glued to the constant: leave it for the
            // next field instead of swallowing it as a delimiter.
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] != pdelim && chars[i] != rdelim {
            i += 1;
        }
        fields.push(chars[start..i].iter().collect());
        if i >= chars.len() {
            terminated = false;
            break;
        }
        let c = chars[i];
        i += 1;
        if c == rdelim {
            terminated = true;
            break;
        }
    }
    (fields, terminated, chars[i.min(chars.len())..].iter().collect())
}

/// Recognise a Hollerith prefix `nH` at `pos`; returns the index one past
/// the `n` content characters (clamped to the end) plus the declared length.
fn hollerith_at(chars: &[char], pos: usize) -> Option<(usize, u64)> {
    let mut j = pos;
    while j < chars.len() && chars[j].is_ascii_digit() && j - pos < 10 {
        j += 1;
    }
    if j == pos || j >= chars.len() || chars[j] != 'H' {
        return None;
    }
    let digits: String = chars[pos..j].iter().collect();
    let n = digits.parse::<u64>().ok()?;
    let width = usize::try_from(n).unwrap_or(usize::MAX);
    let content_start = j + 1;
    let end = content_start.saturating_add(width).min(chars.len());
    Some((end, n))
}

/// When `token` is a Hollerith constant of exactly one character, return it.
pub(crate) fn hollerith_char(token: &str) -> Option<char> {
    let chars: Vec<char> = token.trim().chars().collect();
    let (end, n) = hollerith_at(&chars, 0)?;
    if end == chars.len() && n == 1 {
        chars.last().copied()
    } else {
        None
    }
}

enum Tok {
    Int(i64),
    Real(f64),
    Str(String),
    Bad(String),
    Blank,
}

/// Context-free parameter token classification: Hollerith strings, integers,
/// reals (D or E exponents), blanks — anything else is bad.
fn classify(token: &str) -> Tok {
    let t = token.trim();
    if t.is_empty() {
        return Tok::Blank;
    }
    let chars: Vec<char> = t.chars().collect();
    if let Some((end, n)) = hollerith_at(&chars, 0) {
        let prefix = chars.iter().take_while(|c| c.is_ascii_digit()).count() + 1;
        let available = chars.len() - prefix;
        let fits = usize::try_from(n).is_ok_and(|width| width <= available);
        if end == chars.len() && fits {
            return Tok::Str(chars[prefix..end].iter().collect());
        }
        return Tok::Bad(t.to_string());
    }
    if let Ok(v) = t.parse::<i64>() {
        return Tok::Int(v);
    }
    let promoted = t.replace(['D', 'd'], "E");
    if let Ok(v) = promoted.parse::<f64>() {
        return Tok::Real(v);
    }
    Tok::Bad(t.to_string())
}

/// Human-readable content of a Global field: Hollerith decoded, other text
/// trimmed.
pub(crate) fn decoded(token: &str) -> String {
    match classify(token) {
        Tok::Str(s) => s,
        _ => token.trim().to_string(),
    }
}

/// Build the entity list: pair up the D records, parse the parameter data
/// each one points at, and record how much P data actually belongs to it.
fn build_entities(
    framed: &Framed,
    layout: &mut Layout,
    file: &mut IgesFile,
    sink: &mut Sink<'_>,
) {
    let d_records = framed.dir.len();
    if d_records % 2 == 1 {
        let line = framed.dir_lines.last().copied().unwrap_or(1);
        sink.push(
            Finding::new(
                FindingKind::MalformedEntity,
                Severity::Error,
                format!(
                    "Directory Entry section holds {d_records} records — an odd count leaves \
                     the last entity without its second record"
                ),
            )
            .at(line),
        );
    }
    let fields_of = |rec: &str| -> [String; 9] {
        let chars: Vec<char> = rec.chars().collect();
        std::array::from_fn(|j| {
            chars[8 * j..8 * j + 8].iter().collect::<String>().trim().to_string()
        })
    };

    for pair in 0..d_records / 2 {
        let de = pair as u64 + 1;
        let line1 = framed.dir_lines[2 * pair];
        let line2 = framed.dir_lines[2 * pair + 1];
        let f1 = fields_of(&framed.dir[2 * pair]);
        let f2 = fields_of(&framed.dir[2 * pair + 1]);
        let de_type = f1[0].parse::<u64>().unwrap_or(0);
        let name = display_name(de_type);

        if f1[0].parse::<u64>().is_err() {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {de} has a non-numeric entity type '{}' — expected the \
                         IGES type number",
                        f1[0]
                    ),
                )
                .at(line1),
            );
        }
        match f2[0].parse::<u64>() {
            Ok(t2) if t2 == de_type => {}
            Ok(t2) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {de}: second record type {t2} does not match first \
                         record type {de_type}"
                    ),
                )
                .at(line2)
                .on_entity(name.as_str(), de),
            ),
            Err(_) => sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {de}: second record entity type '{}' is non-numeric",
                        f2[0]
                    ),
                )
                .at(line2)
                .on_entity(name.as_str(), de),
            ),
        }
        if f1[8].chars().any(|c| !(c.is_ascii_digit() || c == ' ')) {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Warning,
                    format!(
                        "directory entry {de} status number '{}' contains characters that are \
                         neither digits nor blanks",
                        f1[8]
                    ),
                )
                .at(line1)
                .on_entity(name.as_str(), de),
            );
        }
        for (raw, slot) in [(&f2[5], 6usize), (&f2[6], 7usize)] {
            if !raw.trim().is_empty() {
                sink.push(
                    Finding::new(
                        FindingKind::UnsupportedConstruct,
                        Severity::Info,
                        format!(
                            "directory entry {de} writes '{raw}' into reserved directory field \
                             {slot}"
                        ),
                    )
                    .at(line2)
                    .on_entity(name.as_str(), de),
                );
            }
        }

        let param_pointer = f1[1].parse::<u64>().unwrap_or(0);
        let pointer_ok = f1[1].parse::<u64>().is_ok();
        if !pointer_ok {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {de} has a non-numeric parameter data pointer '{}' — \
                         expected a P-section sequence number",
                        f1[1]
                    ),
                )
                .at(line1)
                .on_entity(name.as_str(), de),
            );
        }
        let declared = f2[3].parse::<u64>().unwrap_or(0);
        let declared_ok = f2[3].parse::<u64>().is_ok();
        if !declared_ok {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {de} has a non-numeric parameter line count '{}'",
                        f2[3]
                    ),
                )
                .at(line2)
                .on_entity(name.as_str(), de),
            );
        } else if declared < 1 {
            sink.push(
                Finding::new(
                    FindingKind::MalformedEntity,
                    Severity::Error,
                    format!(
                        "directory entry {de} declares {declared} parameter records — at least \
                         one is required"
                    ),
                )
                .at(line2)
                .on_entity(name.as_str(), de),
            );
        }

        // Locate the entity's actual parameter data: the run of P records
        // whose DE pointer names this entity's first D record (2i-1).
        let expected_de = 2 * de - 1;
        let mut span =
            EntitySpan { pointer_ok, declared_ok, declared, records: Vec::new(), first_line: None };
        let ptr = usize::try_from(param_pointer).unwrap_or(usize::MAX);
        if pointer_ok && ptr >= 1 && ptr <= layout.param_records.len() {
            let mut j = ptr - 1;
            while j < layout.param_records.len()
                && layout.param_records[j].de == Some(expected_de)
            {
                span.first_line.get_or_insert(layout.param_records[j].line);
                span.records.push(j);
                j += 1;
            }
        }

        // Parse the spanned record bodies into parameters.
        let mut params: Vec<IgesParam> = Vec::new();
        if let Some(&first) = span.records.first() {
            let first_line = layout.param_records[first].line;
            let body: String =
                span.records.iter().map(|&j| layout.param_records[j].body.as_str()).collect();
            let (tokens, terminated, trailing) =
                split_fields(&body, layout.param_delim, layout.record_delim);
            for tok in &tokens {
                match classify(tok) {
                    Tok::Blank => params.push(IgesParam::Blank),
                    Tok::Int(v) => params.push(IgesParam::Int(v)),
                    Tok::Real(v) => params.push(IgesParam::Real(v)),
                    Tok::Str(s) => params.push(IgesParam::Str(s)),
                    Tok::Bad(raw) => {
                        let why = if raw.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                            "declares more characters than remain — broken Hollerith string"
                        } else {
                            "is not a valid IGES integer, real, or Hollerith string"
                        };
                        sink.push(
                            Finding::new(
                                FindingKind::MalformedEntity,
                                Severity::Error,
                                format!("directory entry {de}: parameter '{raw}' {why}"),
                            )
                            .at(first_line)
                            .on_entity(name.as_str(), de),
                        );
                    }
                }
            }
            if !terminated {
                sink.push(
                    Finding::new(
                        FindingKind::MalformedEntity,
                        Severity::Error,
                        format!(
                            "parameter data for directory entry {de} is never terminated by the \
                             record delimiter '{}'",
                            layout.record_delim
                        ),
                    )
                    .at(first_line)
                    .on_entity(name.as_str(), de),
                );
            } else if !trailing.trim().is_empty() {
                sink.push(
                    Finding::new(
                        FindingKind::MalformedEntity,
                        Severity::Warning,
                        format!(
                            "unexpected data after the record delimiter in the parameter data \
                             of directory entry {de}"
                        ),
                    )
                    .at(first_line)
                    .on_entity(name.as_str(), de),
                );
            }
        }

        file.entities.push(IgesEntity {
            de_number: de,
            de_type,
            param_pointer,
            fields: [f1, f2],
            params,
        });
        layout.spans.push(span);
    }
}
