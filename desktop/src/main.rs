//! TPT CAD Format Pro — desktop shell (paid edition).
//!
//! A native host binary that opens the OS webview (WebView2 on Windows) and
//! serves the `trunk build --release` output of the appfront DOM bundle from
//! `dist/`. The same DOM bundle ships to the hub for the free WASM edition;
//! the Pro difference at runtime is that the bundle is built with
//! `--features pro`, which compiles in the batch/folder-watch controls, and
//! that this host implements the filesystem side natively:
//!
//! - `pick_validate`  — multi-file dialog → full detailed `.txt` report
//!   written beside each file (free UI shows the capped in-browser report).
//! - `pick_convert`   — multi-file dialog → batch conversion to STEP/IGES
//!   for the supported entity subset; outputs written beside the inputs.
//! - `watch_start` / `watch_stop` — polling folder watch: every new or
//!   modified STEP/IGES file is validated (or auto-converted), reported, and
//!   summarised in an OS notification.
//! - `open_external`  — store/docs links out to the real browser.
//!
//! The web→native IPC bridge is fire-and-forget for app actions (replies
//! exist only for built-ins), so the UI states its intent and the native
//! side reports progress through OS notifications.
//!
//! `dist/` is also embedded into the exe at compile time (see [`DIST`]), so
//! the file actually shipped to customers (the Gumroad zip) is the exe
//! alone. Resolve order for the bundle directory:
//! 1. `TPT_CAD_DIST` environment variable (dev override),
//! 2. `./dist` (dev: running from the repo root),
//! 3. `dist` next to the executable,
//! 4. the embedded copy, extracted once into the OS temp directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use include_dir::{include_dir, Dir};
use tpt_appfront_webview::{Acl, AppBuilder, Capability, ParamKind, ParamSpec, WindowConfig};
use tpt_cad_engine::{report::report_text, CadFormat};

const APP_ID: &str = "nz.co.tptsolutions.cad-format-pro";

/// The `trunk build --release` output, embedded at compile time. `build.ps1
/// -Pro` runs the trunk build before this crate, so `../dist` (relative to
/// this crate's `Cargo.toml`) always exists by the time this macro expands.
static DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/../dist");

fn main() {
    let dist_dir = resolve_dist_dir();
    if !dist_dir.join("index.html").exists() {
        eprintln!(
            "tpt-cad-format-pro: no index.html under {} and embedded-asset extraction \
             failed — this build is missing its web bundle",
            dist_dir.display()
        );
        std::process::exit(1);
    }

    let watcher: WatchHandle = WatchHandle::default();
    let watcher = Arc::new(watcher);

    let result = AppBuilder::new(APP_ID)
        .with_window(WindowConfig {
            id: "main".to_string(),
            title: "TPT CAD Format Pro".to_string(),
            width: 1280,
            height: 900,
            dist_dir,
        })
        .with_single_instance(true)
        .with_acl(Acl {
            capabilities: vec![
                Capability {
                    action: "open_external".to_string(),
                    params: vec![string_param("url")],
                },
                Capability {
                    action: "pick_validate".to_string(),
                    params: vec![],
                },
                Capability {
                    action: "pick_convert".to_string(),
                    params: vec![string_param("target")],
                },
                Capability {
                    action: "watch_start".to_string(),
                    params: vec![string_param("path")],
                },
                Capability {
                    action: "watch_stop".to_string(),
                    params: vec![],
                },
            ],
        })
        .run(move |action, params| {
            match action {
                "open_external" => {
                    let url = params.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    // Belt-and-braces on top of the ACL's string-type check:
                    // only ever hand the OS shell a tptsolutions.co.nz URL,
                    // since this is reachable from anything the hosted page's
                    // JS can run.
                    if url.starts_with("https://tptsolutions.co.nz") {
                        let _ = open::that(url);
                    }
                }
                "pick_validate" => pick_and_validate(),
                "pick_convert" => {
                    let target = params
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("step");
                    pick_and_convert(target);
                }
                "watch_start" => {
                    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        notify("TPT CAD Format", "Choose a folder to watch first.");
                    } else if watcher.start(PathBuf::from(path)) {
                        notify(
                            "TPT CAD Format",
                            &format!("Watching {path} for STEP/IGES files."),
                        );
                    } else {
                        notify(
                            "TPT CAD Format",
                            "Could not start the folder watch — does that folder exist?",
                        );
                    }
                }
                "watch_stop" => {
                    watcher.stop();
                    notify("TPT CAD Format", "Folder watch stopped.");
                }
                _ => {}
            }
            Ok(())
        });

    if let Err(error) = result {
        eprintln!("tpt-cad-format-pro: {error:#}");
        std::process::exit(1);
    }
}

fn string_param(name: &str) -> ParamSpec {
    ParamSpec {
        name: name.to_string(),
        required: true,
        kind: ParamKind::String,
        default: None,
    }
}

// ---------------------------------------------------------------------------
// Pick flows
// ---------------------------------------------------------------------------

/// Multi-select dialog → validate each file → write `<name>.cad-report.txt`
/// beside it → one summary notification.
fn pick_and_validate() {
    let Some(paths) = pick_files("Pick STEP/IGES files to validate") else {
        return;
    };
    let mut errors = 0usize;
    let mut reports = 0usize;
    for path in &paths {
        let Some(report) = validate_file(path) else {
            continue;
        };
        if !report.is_valid() {
            errors += 1;
        }
        if write_report(path, &report).is_ok() {
            reports += 1;
        }
    }
    notify(
        "TPT CAD Format — validation finished",
        &format!(
            "{} file(s) checked, {errors} with errors. Detailed reports written beside the \
             files ({reports}).",
            paths.len()
        ),
    );
}

/// Multi-select dialog → convert each file to `target` → write output beside
/// the input → one summary notification.
fn pick_and_convert(target: &str) {
    let Some(target) = parse_target(target) else {
        notify("TPT CAD Format", "Unknown target format.");
        return;
    };
    let Some(paths) = pick_files(&format!(
        "Pick STEP/IGES files to convert to {}",
        target.label()
    )) else {
        return;
    };
    let mut converted = 0usize;
    let mut failed = 0usize;
    for path in &paths {
        match convert_file(path, target) {
            Ok(out_path) => {
                converted += 1;
                let _ = out_path;
            }
            Err(_) => failed += 1,
        }
    }
    notify(
        "TPT CAD Format — conversion finished",
        &format!(
            "{converted} file(s) converted to {}, {failed} failed. Outputs are beside the \
             inputs.",
            target.label()
        ),
    );
}

fn pick_files(title: &str) -> Option<Vec<PathBuf>> {
    let picked = tpt_appfront_webview::open_dialog(&serde_json::json!({
        "title": title,
        "multiple": true,
        "filters": [
            { "name": "CAD exchange files", "extensions": ["step", "stp", "iges", "igs"] },
            { "name": "All files", "extensions": ["*"] }
        ],
    }));
    let files = picked.get("paths").and_then(|v| v.as_array()).cloned()?;
    Some(
        files
            .into_iter()
            .filter_map(|v| v.as_str().map(PathBuf::from))
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Engine wrappers (shared by pick flows and the folder watch)
// ---------------------------------------------------------------------------

fn parse_target(target: &str) -> Option<CadFormat> {
    match target.to_ascii_lowercase().as_str() {
        "step" | "stp" => Some(CadFormat::Step),
        "iges" | "igs" => Some(CadFormat::Iges),
        _ => None,
    }
}

fn is_cad_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("step") | Some("stp") | Some("iges") | Some("igs")
    )
}

fn validate_file(path: &Path) -> Option<tpt_cad_engine::ValidationReport> {
    let bytes = std::fs::read(path).ok()?;
    Some(tpt_cad_engine::validate(&bytes))
}

fn write_report(source: &Path, report: &tpt_cad_engine::ValidationReport) -> std::io::Result<PathBuf> {
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let out = source.with_file_name(format!("{name}.cad-report.txt"));
    std::fs::write(&out, report_text(name, report))?;
    Ok(out)
}

/// Convert one file, writing the output beside the input.
fn convert_file(path: &Path, target: CadFormat) -> Result<PathBuf, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let result = tpt_cad_engine::convert::convert(&bytes, target)
        .map_err(|e| e.to_string())?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let extension = match target {
        CadFormat::Step => "step",
        CadFormat::Iges => "igs",
    };
    let out = path.with_file_name(format!("{stem}-converted.{extension}"));
    std::fs::write(&out, result.output).map_err(|e| e.to_string())?;
    Ok(out)
}

fn notify(title: &str, body: &str) {
    let _ = tpt_appfront_webview::notify(&serde_json::json!({
        "title": title,
        "body": body,
    }));
}

// ---------------------------------------------------------------------------
// Folder watch — polling scan (no watcher-crate dependency; every few seconds
// the folder is rescanned and new/changed CAD files are processed).
// ---------------------------------------------------------------------------

#[derive(Default)]
struct WatchHandle {
    /// The active watch's stop flag. Swapped on restart; the old thread sees
    /// its own flag set and exits.
    stop: std::sync::Mutex<Arc<AtomicBool>>,
}

impl WatchHandle {
    /// Start watching `path`; stops any previous watch first. Returns false
    /// when the folder does not exist.
    fn start(&self, path: PathBuf) -> bool {
        if !path.is_dir() {
            return false;
        }
        self.stop();
        let stop = Arc::new(AtomicBool::new(false));
        *self.stop.lock().unwrap() = Arc::clone(&stop);
        std::thread::spawn(move || {
            watch_loop(path, stop);
        });
        true
    }

    fn stop(&self) {
        if let Ok(guard) = self.stop.lock() {
            guard.store(true, Ordering::SeqCst);
        }
    }
}

fn watch_loop(path: PathBuf, stop: Arc<AtomicBool>) {
    // Baseline: files already in the folder are not re-processed; only files
    // that appear or change after the watch started are.
    let mut seen = HashMap::new();
    scan(&path, &mut seen);

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let mut current = HashMap::new();
        let scanned = scan(&path, &mut current);
        let arrivals: Vec<&PathBuf> = scanned
            .iter()
            .filter(|(p, m)| seen.get(p).copied() != Some(*m))
            .map(|(p, _)| p)
            .collect();
        if arrivals.is_empty() {
            seen = current;
            continue;
        }
        let mut errors = 0usize;
        let mut processed = 0usize;
        for file in &arrivals {
            let Some(report) = validate_file(file) else {
                continue;
            };
            processed += 1;
            if !report.is_valid() {
                errors += 1;
            }
            let _ = write_report(file, &report);
        }
        seen = current;
        if processed > 0 {
            notify(
                "TPT CAD Format — folder watch",
                &format!(
                    "{processed} new file(s) validated, {errors} with errors. Reports are \
                     beside the files."
                ),
            );
        }
    }
}

/// Collect CAD files and their mtimes (secs since the epoch).
fn scan(dir: &Path, out: &mut HashMap<PathBuf, u64>) -> Vec<(PathBuf, u64)> {
    let mut scanned = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return scanned;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() || !is_cad_file(&entry.path()) {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = entry.path();
        out.insert(path.clone(), mtime);
        scanned.push((path, mtime));
    }
    scanned
}

fn resolve_dist_dir() -> PathBuf {
    if let Ok(from_env) = std::env::var("TPT_CAD_DIST") {
        return PathBuf::from(from_env);
    }
    let cwd_dist = PathBuf::from("dist");
    if cwd_dist.join("index.html").exists() {
        return cwd_dist;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let exe_dist = exe_dir.join("dist");
            if exe_dist.join("index.html").exists() {
                return exe_dist;
            }
        }
    }
    extract_embedded_dist()
}

/// Writes the embedded [`DIST`] contents to a stable temp directory (once —
/// subsequent launches reuse the extracted copy) and returns its path. This
/// is the path a customer's real double-click takes: no `dist/` folder
/// shipped or found, so the exe unpacks its own.
fn extract_embedded_dist() -> PathBuf {
    let out_dir = std::env::temp_dir().join(format!("{APP_ID}-dist"));
    if !out_dir.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&out_dir);
        if let Err(error) = DIST.extract(&out_dir) {
            eprintln!("tpt-cad-format-pro: failed to extract embedded assets: {error:#}");
        }
    }
    out_dir
}
