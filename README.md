# TPT CAD Format

Validate and convert industrial CAD/data-exchange files (STEP / IGES) before
they cause problems downstream. Free in-browser edition: validate one file,
report issues, no conversion. Pro desktop edition ($99, Gumroad): batch
conversion between formats, detailed geometry-issue reports, folder-watch
automation.

Hub listing: `tptsolutions.co.nz/tools/cad-format` · Spec: `apps.md` §T1-14 in
the `tpt-electrician-nz-2` repo.

## Layout

```
Cargo.toml      workspace root = the wasm lib crate (tpt-app-cad-format)
src/lib.rs      hub contract: mount(el) + start() (marker-div guarded)
src/app.rs      DOM UI (wasm32 only)
engine/         tpt-cad-engine — validation/conversion adapter over
                tpt-io-standards; plain Rust, host-testable (the moat)
desktop/        (Pro) native webview shell, bin tpt-cad-format-pro
index.html      standalone trunk dev page (#tpt-appfront-root marker div)
build.ps1       canonical build pipeline (Windows)
```

## Hub contract

The web repo's `WasmAppRunner`:

1. dynamic-imports the wasm-bindgen `--target web` glue (`cad-format.js`),
2. awaits its default export (wasm init),
3. calls the named export `mount(container)` with the hub's container div.

Standalone `trunk serve` mounts via the `#tpt-appfront-root` marker div in
`index.html` instead — never `document.body` — so the two hosts never
double-mount.

## Build

```
./build.ps1          # free edition -> <web repo>/public/apps/cad-format/
./build.ps1 -Pro     # pro edition  -> dist\ + release\tpt-cad-format-pro.exe
./build.ps1 -SkipBuild   # re-run wasm-bindgen on an existing build
```

`TPT_WEB_REPO` overrides the web repo path. The free and Pro builds are
separate cargo builds writing to separate directories on purpose — the free
hub bundle must never be built with `--features pro`.

## Test

```
just test    # cargo test -p tpt-cad-engine (samples + unit)
just check   # type-check free and pro wasm builds
```

## Version pins

- `wasm-bindgen = "=0.2.128"` — pinned to the installed wasm-bindgen CLI
  version; the CLI rejects wasm built against a different schema version.
  Bump both together.
- wasm-opt disabled (`data-wasm-opt="0"` + `Trunk.toml`) — the cached Binaryen
  rejects bulk-memory opcodes; the release profile already keeps the payload
  small.
