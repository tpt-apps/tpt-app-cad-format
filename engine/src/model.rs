//! Plain-data model shared by every TPT CAD Format front end (free WASM UI,
//! Pro desktop UI, batch tooling). Both validators — and the Pro converter —
//! speak only in these types.

/// CAD exchange format of an inspected file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CadFormat {
    Step,
    Iges,
}

impl CadFormat {
    /// Human-readable label for reports and UI chips.
    pub fn label(self) -> &'static str {
        match self {
            CadFormat::Step => "STEP (ISO 10303-21)",
            CadFormat::Iges => "IGES 5.x",
        }
    }

    /// Standard file extensions, without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            CadFormat::Step => &["step", "stp"],
            CadFormat::Iges => &["iges", "igs"],
        }
    }

    /// Sniff raw bytes for a recognisable STEP or IGES file.
    ///
    /// STEP files start with the ISO-10303-21 magic. IGES is fixed-format:
    /// 80-column records whose column 73 holds a section letter (S/G/D/P/T)
    /// and whose columns 74-80 hold a right-justified sequence number. The
    /// sniff requires both the letter and a plausible first sequence number
    /// (`       1`) so ordinary text is not misdetected.
    pub fn detect(bytes: &[u8]) -> Option<CadFormat> {
        if bytes.starts_with(b"ISO-10303-21;") {
            return Some(CadFormat::Step);
        }
        // Some writers indent the magic; accept leading whitespace.
        let head = &bytes[..bytes.len().min(64)];
        if let Some(pos) = head.iter().position(|&b| b == b'I') {
            if bytes[pos..].starts_with(b"ISO-10303-21;") {
                return Some(CadFormat::Step);
            }
        }
        if Self::looks_like_iges(bytes) {
            return Some(CadFormat::Iges);
        }
        None
    }

    fn looks_like_iges(bytes: &[u8]) -> bool {
        // Examine only the first few records (Start sections are free text,
        // files can be megabytes).
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]);
        let mut lines = head.lines();
        // First record: section letter 'S' at column 73 (index 72) and a
        // sequence number starting at 1 in columns 74-80.
        let Some(first) = lines.next() else { return false };
        let f = first.as_bytes();
        if f.len() != 80 || f[72] != b'S' {
            return false;
        }
        if !f[73..80].iter().all(|&b| b == b' ' || b.is_ascii_digit())
            || !f[73..80].contains(&b'1')
        {
            return false;
        }
        // Confirm the layout on a later record (G/D/P section letter + seq).
        for line in lines.take(3) {
            let l = line.as_bytes();
            if l.len() == 80 && matches!(l[72], b'S' | b'G' | b'D' | b'P' | b'T') {
                return true;
            }
        }
        false
    }
}

/// Severity of a single finding, ordered Error > Warning > Info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The file is invalid or unreadable; downstream CAD systems will reject
    /// it or silently lose data.
    Error,
    /// The file will probably import, but this aspect is suspect.
    Warning,
    /// Informational detail surfaced in the detailed (Pro) report.
    Info,
}

/// Classification of a single validation finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingKind {
    /// The file could not be parsed at all.
    NotParseable,
    /// An entity record is structurally broken (bad field count, bad enum,
    /// non-numeric data where numbers are required, ...).
    MalformedEntity,
    /// A reference points at an entity that does not exist: a STEP `#id`
    /// with no instance, or an IGES directory-entry / pointer out of range.
    DanglingReference,
    /// IGES section sequence numbers skip or repeat, or D/P pointers are
    /// inconsistent with the section layout.
    SequenceGap,
    /// A required file section is missing (IGES Start/Global/Directory/
    /// Parameter/Terminator; STEP DATA section / ENDSEC).
    MissingSection,
    /// Geometry that parses fine but is degenerate: NaN/inf coordinates,
    /// zero-length direction vectors, non-positive radii.
    DegenerateGeometry,
    /// The file is valid but uses constructs this edition does not analyse
    /// or (Pro) cannot convert; always Info in the free edition.
    UnsupportedConstruct,
    /// The bytes are neither STEP nor IGES.
    UnsupportedFormat,
}

impl FindingKind {
    /// Short human label (report tables, UI badges).
    pub fn label(&self) -> &'static str {
        match self {
            FindingKind::NotParseable => "Not parseable",
            FindingKind::MalformedEntity => "Malformed entity",
            FindingKind::DanglingReference => "Dangling reference",
            FindingKind::SequenceGap => "Sequence gap",
            FindingKind::MissingSection => "Missing section",
            FindingKind::DegenerateGeometry => "Degenerate geometry",
            FindingKind::UnsupportedConstruct => "Unsupported construct",
            FindingKind::UnsupportedFormat => "Unsupported format",
        }
    }
}

/// One validation finding, self-describing for both UIs.
#[derive(Debug, Clone)]
pub struct Finding {
    pub kind: FindingKind,
    pub severity: Severity,
    /// Entity type involved, e.g. `"CARTESIAN_POINT"` or IGES type 110 name.
    pub entity: Option<String>,
    /// STEP instance id or IGES directory-entry number, when known.
    pub reference: Option<u64>,
    /// 1-based line number in the original file, when known.
    pub line: Option<usize>,
    pub message: String,
}

impl Finding {
    /// Convenience constructor: kind + severity + message, no locations.
    pub fn new(kind: FindingKind, severity: Severity, message: impl Into<String>) -> Self {
        Finding {
            kind,
            severity,
            entity: None,
            reference: None,
            line: None,
            message: message.into(),
        }
    }

    /// Builder-style position attachment.
    pub fn at(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    pub fn on_entity(mut self, entity: impl Into<String>, reference: u64) -> Self {
        self.entity = Some(entity.into());
        self.reference = Some(reference);
        self
    }
}

/// Snapshot statistics for an inspected file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileStats {
    /// Detected format, if any.
    pub format: Option<CadFormat>,
    /// STEP `FILE_SCHEMA` name / IGES system version, when readable.
    pub schema: Option<String>,
    /// Total entity instances (STEP) or directory entries (IGES).
    pub entity_count: usize,
    /// Distinct entity types with counts, sorted by count (desc) then name.
    pub entity_types: Vec<(String, usize)>,
    /// Line count of the raw file.
    pub line_count: usize,
}

/// Result of inspecting one file.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// Detected format, if any.
    pub format: Option<CadFormat>,
    pub stats: FileStats,
    /// Findings ordered by severity (Error first), then by line/id.
    pub findings: Vec<Finding>,
}

impl ValidationReport {
    pub fn error_count(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Warning).count()
    }

    pub fn info_count(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Info).count()
    }

    /// True when no Error-severity finding exists.
    pub fn is_valid(&self) -> bool {
        self.error_count() == 0
    }

    /// Sort findings into canonical display order.
    pub fn sort(&mut self) {
        self.findings
            .sort_by(|a, b| a.severity.cmp(&b.severity).then(a.line.cmp(&b.line)));
    }
}

/// Engine-level error for fallible operations (Pro conversion).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CadError {
    /// Input is not a recognisable STEP/IGES file.
    UnsupportedFormat,
    /// The input parsed but the requested operation cannot proceed.
    Invalid(&'static str),
    /// The conversion itself failed mid-way.
    Convert(String),
}

impl std::fmt::Display for CadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CadError::UnsupportedFormat => write!(f, "not a recognisable STEP or IGES file"),
            CadError::Invalid(msg) => write!(f, "invalid input: {msg}"),
            CadError::Convert(msg) => write!(f, "conversion failed: {msg}"),
        }
    }
}

impl std::error::Error for CadError {}
