# TPT CAD Format — Build Todo

> Hub listing: `tptsolutions.co.nz/tools/cad-format` (registry slug
> `cad-format`, category `engineering`). Spec: `apps.md` §T1-14 in the
> tpt-electrician-nz-2 repo. Free WASM edition: validate one file, report
> issues, no conversion. Paid offline edition ($99 USD, Gumroad): batch
> conversion between formats, detailed geometry-issue reports, and
> folder-watch automation. Base wraps `tpt-io-standards`. UI on `tpt-appfront`.
>
> Hub contract: `wasm-bindgen --target web` glue exposes `mount(container)`
> (called by `WasmAppRunner` after `default()` init). Standalone `trunk serve`
> mounts into the `#tpt-appfront-root` marker div instead — never
> `document.body` — so the two hosts never double-mount.

## Phase 0 — Scaffold & pipeline proof
- [x] `tpt-appfront init tpt-app-cad-format --target dom` (path deps on the tpt-appfront checkout)
- [x] Restructure scaffold to hub contract: `#[wasm_bindgen] pub fn mount(el)` + `#[wasm_bindgen(start)]` no-op unless `#tpt-appfront-root` present
- [x] `index.html` carries the `#tpt-appfront-root` marker div
- [x] Build pipeline script mirroring fea-lite/cutlist-optimizer's (`--target web` glue → web repo `public/apps/cad-format/`)
- [x] Hello-world build loads through the standalone dev page (`trunk serve`)
- [x] git init + first commit

## Phase 1 — Solver integration (wrap tpt-io-standards)
- [x] Survey `tpt-io-standards` — no STEP/IGES-specific validation existed;
      `tpt-io-ifc` provides a generic ISO-10303-21 parser; IGES and all
      semantic/geometry checks are owned by this repo's engine
- [x] Adapter crate `engine/`: CAD file bytes in → validation findings
      (issue type, entity reference) out, and (pro) → converted file bytes out
      — as plain structs/byte buffers, no UI deps, host-unit-testable
- [x] Validation against known-good and known-malformed sample STEP/IGES files
      (22 engine tests: 6 STEP + 13 IGES + 3 conversion)

## Phase 2 — Free WASM UI
- [x] Single-file upload, validation-only findings report
- [x] Free-tier upsell copy pointing at batch conversion, detailed reports,
      folder-watch automation
- [x] Browser-verified: mount, upload, findings render correctly
      (malformed → 5 findings w/ entity refs; clean → green verdict +
      entity histogram; junk → rejected)

## Phase 3 — Paid desktop edition (cargo feature `pro`)
- [x] `pro` gates: batch format conversion between STEP/IGES (points, lines,
      circles; skipped types are reported, never silently dropped), detailed
      geometry-issue reports (`<name>.cad-report.txt`), folder-watch
      automation (desktop-only polling watcher)
- [x] Pro UI: batch tools (native multi-select dialogs), conversion controls
      (→ STEP / → IGES), detailed report view (native reports),
      folder-watch toggle with live watch state
- [x] Desktop shell via `tpt-appfront-webview`; Windows exe build + smoke test
      (`release/tpt-cad-format-pro.exe`, 3.2 MB, dist embedded; window up,
      Pro UI verified on screen). NB: stop `trunk serve` before `build.ps1
      -Pro` — the dev server races it for `dist/`.
- [ ] Gumroad product from apps.md listing copy ($99), exe upload, URL into
      registry — MANUAL STEP; listing copy at apps.md §T1-14 / `GUMROAD.md`

## Phase 4 — Ship & measure
- [x] Registry entry (`engineering` category, `wasm: { entry: '/apps/cad-format/cad-format.js' }`)
      added to `tpt-apps-registry.ts` with `status: 'coming-soon'` + proFeatures
- [x] Hub card/detail/sitemap verified in dev (`/tools`, `/tools/cad-format`,
      bundle assets 200, sitemap includes cad-format). Flip to `'live'` once
      the Gumroad listing exists; prod check after deploy
- [x] WASM size budget on the shipped build: 117.9 KB gzipped (budget 2 MB)
