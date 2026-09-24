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
- [ ] `tpt-appfront init tpt-app-cad-format --target dom` (path deps on the tpt-appfront checkout)
- [ ] Restructure scaffold to hub contract: `#[wasm_bindgen] pub fn mount(el)` + `#[wasm_bindgen(start)]` no-op unless `#tpt-appfront-root` present
- [ ] `index.html` carries the `#tpt-appfront-root` marker div
- [ ] Build pipeline script mirroring fea-lite/cutlist-optimizer's (`--target web` glue → web repo `public/apps/cad-format/`)
- [ ] Hello-world build loads through the standalone dev page (`trunk serve`)
- [ ] git init + first commit

## Phase 1 — Solver integration (wrap tpt-io-standards)
- [ ] Survey `tpt-io-standards` for STEP/IGES parsing and validation support
      (geometry issue classes: gaps, self-intersections, malformed entities)
- [ ] Adapter crate `engine/`: CAD file bytes in → validation findings
      (issue type, entity reference) out, and (pro) → converted file bytes out
      — as plain structs/byte buffers, no UI deps, host-unit-testable
- [ ] Validation against known-good and known-malformed sample STEP/IGES files

## Phase 2 — Free WASM UI
- [ ] Single-file upload, validation-only findings report
- [ ] Free-tier upsell copy pointing at batch conversion, detailed reports,
      folder-watch automation
- [ ] Browser-verified: mount, upload, findings render correctly

## Phase 3 — Paid desktop edition (cargo feature `pro`)
- [ ] `pro` gates: batch format conversion between STEP/IGES, detailed
      geometry-issue reports, folder-watch automation (desktop-only, needs
      filesystem watch support)
- [ ] Pro UI: batch file list, conversion controls, detailed report view,
      folder-watch toggle
- [ ] Desktop shell via `tpt-appfront-webview`; Windows exe build + smoke test
- [ ] Gumroad product from apps.md listing copy ($49), exe upload, URL into
      registry — manual step, listing copy at apps.md §T1-14

## Phase 4 — Ship & measure
- [ ] Registry entry (`engineering` category, `wasm: { entry: '/apps/cad-format/cad-format.js' }`)
      per the "Adding a finished app to the hub" runbook at the bottom of apps.md
- [ ] Verify hub card/detail/sitemap/runner in dev, then prod after deploy
- [ ] WASM size budget check on the shipped build
