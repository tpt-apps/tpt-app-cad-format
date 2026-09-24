//! Detailed text reports (Pro). Renders a [`ValidationReport`] as the
//! human-readable `.txt` written beside inspected files by the desktop
//! edition's pick-and-validate and folder-watch automation.

use crate::model::{CadFormat, Severity, ValidationReport};

/// Render the full detailed report. Unlike the free UI (which caps findings
/// per class for rendering), this is complete: every finding, every entity
/// type, all stats.
pub fn report_text(file_name: &str, report: &ValidationReport) -> String {
    let mut out = String::new();
    out.push_str("TPT CAD Format — detailed validation report\n");
    out.push_str("==========================================\n\n");
    out.push_str(&format!("File:      {file_name}\n"));
    out.push_str(&format!(
        "Format:    {}\n",
        report
            .format
            .map(CadFormat::label)
            .unwrap_or("Unrecognised")
    ));
    if let Some(schema) = &report.stats.schema {
        out.push_str(&format!("Schema:    {schema}\n"));
    }
    out.push_str(&format!("Entities:  {}\n", report.stats.entity_count));
    out.push_str(&format!("Lines:     {}\n", report.stats.line_count));
    out.push_str(&format!(
        "Findings:  {} error(s), {} warning(s), {} note(s)\n",
        report.error_count(),
        report.warning_count(),
        report.info_count()
    ));
    out.push_str(&format!(
        "Verdict:   {}\n",
        if report.is_valid() {
            "no errors — file should import cleanly"
        } else {
            "ERRORS FOUND — importing systems will reject this file or drop data"
        }
    ));

    if !report.stats.entity_types.is_empty() {
        out.push_str("\nEntity types\n------------\n");
        for (name, count) in &report.stats.entity_types {
            out.push_str(&format!("  {count:>6}  {name}\n"));
        }
    }

    if !report.findings.is_empty() {
        out.push_str(
            "\nFindings (ordered by severity)\n------------------------------\n",
        );
        for (index, finding) in report.findings.iter().enumerate() {
            let severity = match finding.severity {
                Severity::Error => "ERROR",
                Severity::Warning => "WARN ",
                Severity::Info => "NOTE ",
            };
            out.push_str(&format!("\n{:>4}. [{severity}] {}", index + 1, finding.kind.label()));
            if let Some(entity) = &finding.entity {
                out.push_str(&format!(" — {entity}"));
                if let Some(id) = finding.reference {
                    out.push_str(&format!(" #{id}"));
                }
            } else if let Some(id) = finding.reference {
                out.push_str(&format!(" — #{id}"));
            }
            if let Some(line) = finding.line {
                out.push_str(&format!(" (line {line})"));
            }
            out.push('\n');
            out.push_str(&format!("      {}\n", finding.message.replace('\n', "\n       ")));
        }
    }

    out.push_str("\nGenerated locally by TPT CAD Format. Nothing was uploaded.\n");
    out
}
