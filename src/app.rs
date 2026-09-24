// DOM UI for TPT CAD Format — free in-browser edition.
//
// Architecture mirrors tpt-app-fea-lite: the static shell (header, upload
// zone placeholders, status line, results placeholder) is mounted exactly
// once (`tpt_appfront_dom::mount`) so typing/listeners never churn; the
// results section re-renders fine-grained via `tpt_appfront_dom::render`
// driven by the `outcome` Signal; the file input is raw web-sys DOM because
// UITree has no file-input node kind.
//
// Validation itself runs in the engine crate (`tpt_cad_engine::validate`) —
// plain Rust, shared verbatim with the Pro desktop edition.
use std::cell::RefCell;
use std::rc::Rc;
use tpt_appfront_core::{Signal, UITree};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Where the upsell points until the Gumroad product URL exists (then this
/// becomes the gum.co link — GUMROAD.md step 7).
const STORE_URL: &str = "https://tptsolutions.co.nz/tools/cad-format";

#[derive(Debug, Clone)]
enum Msg {
    /// Open the Pro store link (protocol-aware: browser tab vs desktop IPC).
    OpenStore,
}

thread_local! {
    /// The file picked in the raw `<input type="file">`, waiting for Validate.
    static SELECTED_FILE: RefCell<Option<web_sys::File>> = const { RefCell::new(None) };
    /// The Validate button, to disable while a file is being read.
    static VALIDATE_BTN: RefCell<Option<web_sys::HtmlButtonElement>> = const { RefCell::new(None) };
    /// Shared status-line signal (created in mount_app).
    static STATUS: RefCell<Option<Signal<String>>> = const { RefCell::new(None) };
    /// Shared outcome signal (created in mount_app).
    static OUTCOME: RefCell<Option<Signal<Option<ReportView>>>> = const { RefCell::new(None) };
}

pub fn mount_app(container: &web_sys::Element) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window.document().ok_or_else(|| JsValue::from_str("no document"))?;

    upsert_style(&document, "cad-format-styles", BASE_CSS)?;
    container.class_list().add_1("cad-app")?;

    let status = Signal::new("Choose a STEP or IGES file to validate.".to_string());
    let outcome: Signal<Option<ReportView>> = Signal::new(None);
    STATUS.with(|cell| *cell.borrow_mut() = Some(status.clone()));
    OUTCOME.with(|cell| *cell.borrow_mut() = Some(outcome.clone()));

    // 1. Static shell — mounted exactly once.
    let dispatch: Rc<dyn Fn(Msg)> = Rc::new(|msg| match msg {
        Msg::OpenStore => open_external(STORE_URL),
    });
    let root = tpt_appfront_dom::mount(container, &shell_tree(), Rc::clone(&dispatch))?;
    std::mem::forget(root);

    // 2. Reactive status line into its placeholder. Same render path as the
    //    results section (fine-grained render effect), not reactive_text —
    //    one rendering mechanism for every signal-driven section.
    let status_el = container
        .query_selector(".cad-statusline")?
        .ok_or_else(|| JsValue::from_str("status placeholder missing"))?;
    let status_for_view = status.clone();
    let handle = tpt_appfront_dom::render(
        &status_el,
        Rc::new(move || {
            let text = status_for_view.get();
            UITree::container(|c| { c.text(text.clone()); })
        }),
        dispatch.clone(),
    )?;
    std::mem::forget(handle);

    // 3. Reactive results section into its placeholder.
    let results_el = container
        .query_selector(".cad-results")?
        .ok_or_else(|| JsValue::from_str("results placeholder missing"))?;
    let handle = tpt_appfront_dom::render(
        &results_el,
        Rc::new(move || results_view(outcome.get())),
        dispatch.clone(),
    )?;
    std::mem::forget(handle);

    // 4. Raw upload controls (file input is not a UITree node kind).
    let upload_el = container
        .query_selector(".cad-upload")?
        .ok_or_else(|| JsValue::from_str("upload placeholder missing"))?;
    build_upload_row(&document, &upload_el)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Static shell
// ---------------------------------------------------------------------------

fn shell_tree() -> UITree<Msg> {
    UITree::container(|c| {
        c.container(|header| {
            header.container(|brand| { brand.text("CAD"); }).class("cad-brand");
            header.container(|titles| {
                titles.container(|row| {
                    row.heading(2, "TPT CAD Format");
                    // Pro build shows a chip instead of the free chip.
                    #[cfg(not(feature = "pro"))]
                    text_span(row, "Free edition", "cad-chip");
                    #[cfg(feature = "pro")]
                    text_span(row, "Pro", "cad-chip");
                })
                .class("cad-title-row");
                text_span(
                    titles,
                    "Validate STEP and IGES exchange files before they cause problems \
                     downstream.",
                    "cad-sub",
                );
            })
            .class("cad-title-group");
        })
        .class("cad-header");

        c.container(|_| {}).class("cad-upload");

        c.container(|_| {}).class("cad-statusline");

        c.container(|_| {}).class("cad-results");

        #[cfg(not(feature = "pro"))]
        upsell_card(c);
    })
}

/// Styled text: UITree text nodes are bare DOM text nodes and cannot carry a
/// class, so anything styled is wrapped in a one-child container.
fn text_span(
    c: &mut tpt_appfront_core::ContainerBuilder<Msg>,
    text: impl Into<String>,
    class: &'static str,
) {
    c.container(|el| { el.text(text.into()); }).class(class);
}

/// Free-build upsell; the Pro build ships the real controls instead.
#[cfg(not(feature = "pro"))]
fn upsell_card(c: &mut tpt_appfront_core::ContainerBuilder<Msg>) {
    c.container(|card| {
        card.heading(3, "Need batch conversion, detailed reports, or folder-watch?");
        card.text(
            "The Pro edition ($99 USD, Windows desktop) adds batch format conversion between \
             STEP and IGES, detailed per-entity geometry-issue reports, and folder-watch \
             automation that validates files as they arrive.",
        );
        card.button("Get the Pro edition — $99")
            .on_click(Msg::OpenStore)
            .class("cad-btn cad-primary");
    })
    .class("cad-card cad-upsell");
}

// ---------------------------------------------------------------------------
// Upload row (raw DOM)
// ---------------------------------------------------------------------------

fn build_upload_row(document: &web_sys::Document, parent: &web_sys::Element) -> Result<(), JsValue> {
    let zone = document.create_element("div")?;
    zone.set_class_name("cad-card cad-dropzone");

    let input: web_sys::HtmlInputElement = document
        .create_element("input")?
        .unchecked_into::<web_sys::HtmlInputElement>();
    input.set_type("file");
    input.set_accept(".step,.stp,.iges,.igs");
    input.set_id("cad-file-input");
    input.class_list().add_1("cad-file-input")?;

    let label: web_sys::HtmlElement = document
        .create_element("label")?
        .unchecked_into::<web_sys::HtmlElement>();
    label.set_attribute("for", "cad-file-input")?;
    label.set_class_name("cad-file-label");
    label.set_text_content(Some("Choose file…"));

    let hint = document.create_element("div")?;
    hint.set_class_name("cad-hint");
    hint.set_text_content(Some("Accepted: .step, .stp, .iges, .igs — everything runs locally \
                                in your browser; nothing is uploaded."));

    zone.append_child(&label)?;
    zone.append_child(&input)?;
    zone.append_child(&hint)?;

    // File picked: remember it and reflect the name in the status line.
    let input_for_change = input.clone();
    let on_change: Closure<dyn Fn()> = Closure::wrap(Box::new(move || {
        let picked = input_for_change
            .files()
            .and_then(|list| list.get(0));
        if let Some(file) = picked {
            let name = file.name();
            let size = human_size(file.size());
            SELECTED_FILE.with(|cell| *cell.borrow_mut() = Some(file));
            set_status(format!("{name} selected ({size}) — press Validate."));
            clear_outcome();
        }
    }));
    input
        .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())?;
    on_change.forget();

    let button: web_sys::HtmlButtonElement = document
        .create_element("button")?
        .unchecked_into::<web_sys::HtmlButtonElement>();
    button.set_type("button");
    button.set_class_name("cad-btn cad-primary");
    button.set_text_content(Some("Validate file"));
    let on_click: Closure<dyn Fn()> = Closure::wrap(Box::new(move || {
        dispatch_validate();
    }));
    button
        .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())?;
    on_click.forget();
    VALIDATE_BTN.with(|cell| *cell.borrow_mut() = Some(button.clone()));

    let row = document.create_element("div")?;
    row.set_class_name("cad-upload-row");
    row.append_child(&zone)?;
    row.append_child(&button)?;

    parent.append_child(&row)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Validation flow
// ---------------------------------------------------------------------------

fn set_status(text: String) {
    STATUS.with(|cell| {
        if let Some(status) = cell.borrow().as_ref() {
            status.set(text);
        }
    });
}

fn set_outcome(view: Option<ReportView>) {
    OUTCOME.with(|cell| {
        if let Some(outcome) = cell.borrow().as_ref() {
            outcome.set(view);
        }
    });
}

fn clear_outcome() {
    set_outcome(None);
}

fn set_busy(busy: bool) {
    VALIDATE_BTN.with(|cell| {
        if let Some(button) = cell.borrow().as_ref() {
            button.set_disabled(busy);
            button.set_text_content(if busy { Some("Validating…") } else { Some("Validate file") });
        }
    });
}

fn dispatch_validate() {
    let file = SELECTED_FILE.with(|cell| cell.borrow().clone());
    match file {
        None => set_status("Choose a STEP or IGES file first.".to_string()),
        Some(file) => read_and_validate(file),
    }
}

fn read_and_validate(file: web_sys::File) {
    set_busy(true);
    set_status(format!("Reading {}…", file.name()));
    clear_outcome();

    let name = file.name();
    let size = file.size();
    let reader = match web_sys::FileReader::new() {
        Ok(reader) => reader,
        Err(_) => {
            set_busy(false);
            set_status("FileReader unavailable in this browser.".to_string());
            return;
        }
    };

    let on_load: Closure<dyn Fn()> = Closure::wrap(Box::new(move || {
        let bytes = READER
            .with(|cell| {
                cell.borrow().as_ref().and_then(|r| r.result().ok())
            })
            .map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec())
            .unwrap_or_default();
        run_validation(&name, size, &bytes);
        set_busy(false);
    }));

    let on_error: Closure<dyn Fn()> = Closure::wrap(Box::new(move || {
        set_status("Could not read the file — try re-saving it and picking it again."
            .to_string());
        set_busy(false);
    }));

    // Keep the reader and closures alive for the lifetime of the page.
    READER.with(|cell| *cell.borrow_mut() = Some(reader.clone()));
    ON_LOAD.with(|cell| *cell.borrow_mut() = Some(on_load));
    ON_ERROR.with(|cell| *cell.borrow_mut() = Some(on_error));

    ON_LOAD.with(|cell| {
        if let Some(closure) = cell.borrow().as_ref() {
            reader.set_onload(Some(closure.as_ref().unchecked_ref()));
        }
    });
    ON_ERROR.with(|cell| {
        if let Some(closure) = cell.borrow().as_ref() {
            reader.set_onerror(Some(closure.as_ref().unchecked_ref()));
        }
    });

    if reader.read_as_array_buffer(&file).is_err() {
        set_busy(false);
        set_status("The browser refused to read that file.".to_string());
    }
}

thread_local! {
    static READER: RefCell<Option<web_sys::FileReader>> = const { RefCell::new(None) };
    static ON_LOAD: RefCell<Option<Closure<dyn Fn()>>> = const { RefCell::new(None) };
    static ON_ERROR: RefCell<Option<Closure<dyn Fn()>>> = const { RefCell::new(None) };
}

fn run_validation(name: &str, size: f64, bytes: &[u8]) {
    set_status(format!("Validating {} ({} entities scanned)…", name, "?".to_string()));
    let report = tpt_cad_engine::validate(bytes);
    set_status(format!(
        "{} validated ({}) — {} error(s), {} warning(s), {} note(s).",
        name,
        human_size(size),
        report.error_count(),
        report.warning_count(),
        report.info_count(),
    ));
    set_outcome(Some(ReportView::new(name.to_string(), size, &report)));
}

// ---------------------------------------------------------------------------
// Results view model + rendering
// ---------------------------------------------------------------------------

/// Pre-rendered report snapshot (strings only) so the reactive render closure
/// clones cheap data instead of the whole engine report.
#[derive(Debug, Clone)]
struct ReportView {
    file_name: String,
    file_size: String,
    format: String,
    schema: String,
    entity_count: usize,
    line_count: usize,
    errors: usize,
    warnings: usize,
    infos: usize,
    is_valid: bool,
    entity_rows: Vec<Vec<String>>,
    entity_more: usize,
    findings: Vec<FindingView>,
}

#[derive(Debug, Clone)]
struct FindingView {
    severity: &'static str,
    severity_class: &'static str,
    kind: String,
    entity: String,
    line: String,
    message: String,
}

impl ReportView {
    fn new(file_name: String, size: f64, report: &tpt_cad_engine::ValidationReport) -> Self {
        let severity_label = |sev: tpt_cad_engine::Severity| match sev {
            tpt_cad_engine::Severity::Error => ("Error", "cad-sev-error"),
            tpt_cad_engine::Severity::Warning => ("Warning", "cad-sev-warning"),
            tpt_cad_engine::Severity::Info => ("Note", "cad-sev-info"),
        };
        let findings = report
            .findings
            .iter()
            .map(|f| {
                let (severity, severity_class) = severity_label(f.severity);
                FindingView {
                    severity,
                    severity_class,
                    kind: f.kind.label().to_string(),
                    entity: match (f.entity.as_deref(), f.reference) {
                        (Some(entity), Some(id)) => format!("{entity} #{id}"),
                        (Some(entity), None) => entity.to_string(),
                        (None, Some(id)) => format!("#{id}"),
                        (None, None) => String::new(),
                    },
                    line: f.line.map(|l| format!("line {l}")).unwrap_or_default(),
                    message: f.message.clone(),
                }
            })
            .collect();

        let show = report.stats.entity_types.iter().take(8);
        let shown = show.count();
        let entity_rows = report
            .stats
            .entity_types
            .iter()
            .take(8)
            .map(|(name, count)| vec![name.clone(), count.to_string()])
            .collect();
        let entity_more = report.stats.entity_types.len().saturating_sub(shown);

        ReportView {
            file_name,
            file_size: human_size(size),
            format: report
                .format
                .map(|f| f.label().to_string())
                .unwrap_or_else(|| "Unrecognised format".to_string()),
            schema: report.stats.schema.clone().unwrap_or_else(|| "—".to_string()),
            entity_count: report.stats.entity_count,
            line_count: report.stats.line_count,
            errors: report.error_count(),
            warnings: report.warning_count(),
            infos: report.info_count(),
            is_valid: report.is_valid(),
            entity_rows,
            entity_more,
            findings,
        }
    }
}

fn results_view(outcome: Option<ReportView>) -> UITree<Msg> {
    let Some(view) = outcome else {
        return UITree::container(|c| {
            c.container(|empty| {
                empty.text(
                    "No report yet — pick a STEP or IGES file above and press Validate. The \
                     file is checked locally in your browser and never uploaded.",
                );
            })
            .class("cad-card cad-empty");
        });
    };

    UITree::container(|c| {
        // Verdict banner.
        c.container(|banner| {
            if view.format == "Unrecognised format" {
                text_span(banner, "Not a STEP or IGES file", "cad-verdict-title");
                banner.text(
                    "The bytes don't carry the ISO-10303-21 marker or the IGES fixed-column \
                     layout. Double-check the export format.",
                );
            } else if view.is_valid {
                text_span(
                    banner,
                    if view.warnings == 0 {
                        "No issues found"
                    } else {
                        "No errors found"
                    },
                    "cad-verdict-title",
                );
                banner.text(format!(
                    "{} parsed cleanly{} — it should import without losing data.",
                    view.format,
                    if view.warnings == 0 {
                        String::new()
                    } else {
                        format!(", but {} suspect detail(s) deserve a look", view.warnings)
                    },
                ));
            } else {
                text_span(
                    banner,
                    format!(
                        "{} error(s) found — this file will cause problems downstream",
                        view.errors
                    ),
                    "cad-verdict-title",
                );
                banner.text(format!(
                    "Importing systems will reject it or silently drop data. Fix the items \
                     below and re-export{}.",
                    if view.warnings > 0 {
                        format!(" (plus {} warning(s) to review)", view.warnings)
                    } else {
                        String::new()
                    }
                ));
            }
        })
        .class(if view.is_valid {
            "cad-card cad-verdict cad-verdict-ok"
        } else {
            "cad-card cad-verdict cad-verdict-bad"
        });

        // File facts.
        c.container(|facts| {
            facts.heading(3, "File summary");
            facts.container(|grid| {
                fact(grid, "File", format!("{} ({})", view.file_name, view.file_size));
                fact(grid, "Format", view.format.clone());
                fact(grid, "Schema / setup", view.schema.clone());
                fact(grid, "Entities", view.entity_count.to_string());
                fact(grid, "Lines", view.line_count.to_string());
                fact(
                    grid,
                    "Findings",
                    format!(
                        "{} error(s), {} warning(s), {} note(s)",
                        view.errors, view.warnings, view.infos
                    ),
                );
            })
            .class("cad-facts");
        })
        .class("cad-card");

        // Findings.
        if !view.findings.is_empty() {
            c.container(|list_card| {
                list_card.heading(3, "Findings");
                text_span(
                    list_card,
                    "Ordered by severity. Entity ids reference the STEP instance numbers \
                     (#n) shown by most CAD systems.",
                    "cad-hint",
                );
                for finding in &view.findings {
                    list_card
                        .container(|row| {
                            row.container(|head| {
                                text_span(head, finding.severity, "cad-sev-badge");
                                text_span(head, finding.kind.clone(), "cad-kind");
                                if !finding.entity.is_empty() {
                                    text_span(head, finding.entity.clone(), "cad-entity");
                                }
                                if !finding.line.is_empty() {
                                    text_span(head, finding.line.clone(), "cad-line");
                                }
                            })
                            .class("cad-finding-head");
                            text_span(row, finding.message.clone(), "cad-finding-msg");
                        })
                        .class(format!("cad-finding {}", finding.severity_class).as_str());
                }
            })
            .class("cad-card");
        }

        // Entity histogram.
        if !view.entity_rows.is_empty() {
            c.container(|types_card| {
                types_card.heading(3, "Entity types");
                types_card
                    .data_grid(
                        ["Entity type", "Count"],
                        view.entity_rows.iter().cloned(),
                    )
                    .class("cad-grid");
                if view.entity_more > 0 {
                    text_span(
                        types_card,
                        format!("… and {} more distinct type(s)", view.entity_more),
                        "cad-hint",
                    );
                }
            })
            .class("cad-card");
        }
    })
}

fn fact(grid: &mut tpt_appfront_core::ContainerBuilder<Msg>, label: &str, value: String) {
    grid.container(|item| {
        text_span(item, label.to_string(), "cad-fact-label");
        text_span(item, value, "cad-fact-value");
    })
    .class("cad-fact");
}

// ---------------------------------------------------------------------------
// Pro store link (protocol-aware)
// ---------------------------------------------------------------------------

/// Open an external URL: desktop shells route through the `__appfront` IPC
/// (`open_external`) so the webview doesn't navigate away; browsers open a
/// new tab. Sniffs the hosting context at runtime — it's the host that
/// matters, not the build (same rule as fea-lite).
fn open_external(url: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let is_desktop = window
        .location()
        .protocol()
        .map(|p| p == "app:")
        .unwrap_or(false);
    if is_desktop {
        let host = js_sys::Reflect::get(&window, &JsValue::from_str("__appfront")).ok();
        let post = host
            .as_ref()
            .and_then(|host| js_sys::Reflect::get(host, &JsValue::from_str("post")).ok())
            .and_then(|post| post.dyn_into::<js_sys::Function>().ok());
        if let (Some(post), Some(host)) = (post, host.clone()) {
            let params = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &params,
                &JsValue::from_str("url"),
                &JsValue::from_str(url),
            );
            let _ = post.call2(
                &host,
                &JsValue::from_str("open_external"),
                &params.into(),
            );
            return;
        }
    }
    let _ = window.open_with_url_and_target(url, "_blank");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn human_size(bytes: f64) -> String {
    let (value, unit) = if bytes >= 1_048_576.0 {
        (bytes / 1_048_576.0, "MB")
    } else if bytes >= 1024.0 {
        (bytes / 1024.0, "KB")
    } else {
        (bytes, "B")
    };
    if unit == "B" {
        format!("{} {unit}", bytes as u64)
    } else {
        format!("{value:.1} {unit}")
    }
}

fn upsert_style(
    document: &web_sys::Document,
    id: &str,
    css: &'static str,
) -> Result<(), JsValue> {
    if document.get_element_by_id(id).is_some() {
        return Ok(());
    }
    let style = document.create_element("style")?;
    style.set_id(id);
    style.set_text_content(Some(css));
    document
        .head()
        .ok_or_else(|| JsValue::from_str("no head"))?
        .append_child(&style)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Styles — light/dark via CSS variables, same conventions as fea-lite.
// Accent is steel blue to distinguish CAD Format from FEA Lite's teal.
// ---------------------------------------------------------------------------

const BASE_CSS: &str = r#"
.cad-app{--cad-bg:#f7f8fa;--cad-surface:#ffffff;--cad-panel:#f2f4f7;--cad-fg:#181d27;--cad-muted:#667085;--cad-border:#e3e7ee;--cad-accent:#2f6fb3;--cad-accent-2:#245a94;--cad-accent-soft:#e8f1fa;--cad-accent-fg:#ffffff;--cad-err:#b3261e;--cad-err-bg:#fbeeec;--cad-warn:#955f00;--cad-warn-bg:#fdf6e3;--cad-info:#57606a;--cad-info-bg:#f3f5f7;--cad-ok:#1a7f37;--cad-ok-bg:#eaf6ee;--cad-shadow:0 1px 2px rgba(16,24,40,.04),0 1px 3px rgba(16,24,40,.06);max-width:980px;margin:0 auto;color:var(--cad-fg);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:14.5px;line-height:1.55;padding:0 4px 32px}
@media(prefers-color-scheme:dark){.cad-app{--cad-bg:#0b0f16;--cad-surface:#141a24;--cad-panel:#161d2a;--cad-fg:#e8ebf0;--cad-muted:#98a2b3;--cad-border:#262f3d;--cad-accent:#5b9bd8;--cad-accent-2:#7fb3e6;--cad-accent-soft:#12243a;--cad-accent-fg:#04121f;--cad-err:#f2b8b5;--cad-err-bg:#2c1a1c;--cad-warn:#e2b93b;--cad-warn-bg:#2b2415;--cad-info:#98a2b3;--cad-info-bg:#1a212c;--cad-ok:#57c98a;--cad-ok-bg:#12281c;--cad-shadow:0 1px 2px rgba(0,0,0,.3)}}
.cad-app *{box-sizing:border-box}
.cad-app h2{font-size:1.36rem;margin:0;font-weight:700;letter-spacing:-.01em}
.cad-app h3{font-size:1rem;margin:0 0 8px;font-weight:700}
.cad-header{display:flex;align-items:flex-start;gap:14px;padding:22px 2px 18px;border-bottom:1px solid var(--cad-border);margin-bottom:22px}
.cad-brand{flex:0 0 auto;width:42px;height:42px;border-radius:11px;background:linear-gradient(135deg,var(--cad-accent),var(--cad-accent-2));display:flex;align-items:center;justify-content:center;color:var(--cad-accent-fg);font-weight:800;font-size:.72rem;letter-spacing:.02em;box-shadow:var(--cad-shadow)}
.cad-title-group{flex:1;min-width:0}
.cad-title-row{display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.cad-chip{display:inline-flex;align-items:center;padding:3px 10px;border-radius:999px;font-size:.66rem;font-weight:800;letter-spacing:.05em;text-transform:uppercase;background:var(--cad-accent-soft);color:var(--cad-accent)}
.cad-sub{color:var(--cad-muted);margin:5px 0 0;font-size:.88rem}
.cad-app>div{display:flex;flex-direction:column;gap:16px}
.cad-card{background:var(--cad-surface);border:1px solid var(--cad-border);border-radius:14px;padding:16px 18px;box-shadow:var(--cad-shadow)}
.cad-upload-row{display:flex;flex-direction:column;gap:14px}
.cad-dropzone{display:flex;flex-direction:column;gap:10px}
.cad-file-input{color:var(--cad-fg);font:inherit}
.cad-file-input::file-selector-button{padding:8px 14px;margin-right:12px;border:1px solid var(--cad-border);border-radius:9px;background:var(--cad-panel);color:var(--cad-fg);font:inherit;font-weight:600;cursor:pointer}
.cad-file-input::file-selector-button:hover{border-color:var(--cad-accent);color:var(--cad-accent)}
.cad-file-label{display:none}
.cad-hint{color:var(--cad-muted);font-size:.8rem;margin:0}
.cad-btn{padding:10px 18px;border-radius:9px;border:1px solid var(--cad-border);background:var(--cad-surface);color:var(--cad-fg);font:inherit;font-weight:600;cursor:pointer;transition:border-color .12s,color .12s,filter .12s;align-self:flex-start}
.cad-btn:hover{border-color:var(--cad-accent);color:var(--cad-accent)}
.cad-btn:disabled{opacity:.55;cursor:progress}
.cad-primary{background:linear-gradient(135deg,var(--cad-accent),var(--cad-accent-2));border-color:transparent;color:var(--cad-accent-fg);box-shadow:var(--cad-shadow)}
.cad-primary:hover{color:var(--cad-accent-fg);filter:brightness(1.08)}
.cad-statusline{min-height:1.4em;color:var(--cad-muted);font-size:.9rem;padding:0 2px}
.cad-empty{color:var(--cad-muted)}
.cad-verdict-ok{border-color:color-mix(in srgb,var(--cad-ok) 35%,var(--cad-border));background:var(--cad-ok-bg)}
.cad-verdict-bad{border-color:color-mix(in srgb,var(--cad-err) 35%,var(--cad-border));background:var(--cad-err-bg)}
.cad-verdict-title{font-weight:800;font-size:1.02rem;margin-bottom:4px}
.cad-facts{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:10px 18px}
.cad-fact{display:flex;flex-direction:column;gap:2px}
.cad-fact-label{font-size:.7rem;color:var(--cad-muted);font-weight:700;text-transform:uppercase;letter-spacing:.04em}
.cad-fact-value{font-weight:600;font-size:.92rem;overflow-wrap:anywhere}
.cad-finding{display:flex;flex-direction:column;gap:5px;padding:11px 13px;border:1px solid var(--cad-border);border-radius:10px;margin-top:9px}
.cad-finding-head{display:flex;align-items:center;gap:9px;flex-wrap:wrap}
.cad-sev-badge{display:inline-flex;padding:2px 9px;border-radius:999px;font-size:.66rem;font-weight:800;letter-spacing:.04em;text-transform:uppercase}
.cad-sev-error .cad-sev-badge{background:var(--cad-err);color:#fff}
@media(prefers-color-scheme:dark){.cad-sev-error .cad-sev-badge{color:#1c0b0a}}
.cad-sev-warning .cad-sev-badge{background:var(--cad-warn-bg);color:var(--cad-warn);border:1px solid var(--cad-warn)}
.cad-sev-info .cad-sev-badge{background:var(--cad-info-bg);color:var(--cad-info);border:1px solid var(--cad-border)}
.cad-finding.cad-sev-error{border-left:3px solid var(--cad-err)}
.cad-finding.cad-sev-warning{border-left:3px solid var(--cad-warn)}
.cad-finding.cad-sev-info{border-left:3px solid var(--cad-info)}
.cad-grid{width:100%;border-collapse:collapse;font-size:.88rem}
.cad-kind{font-weight:700;font-size:.88rem}
.cad-entity{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.8rem;background:var(--cad-panel);border-radius:6px;padding:2px 7px}
.cad-line{color:var(--cad-muted);font-size:.78rem}
.cad-finding-msg{font-size:.9rem}
.cad-upsell{border-style:dashed}
.cad-upsell .cad-btn{margin-top:10px}
"#;
