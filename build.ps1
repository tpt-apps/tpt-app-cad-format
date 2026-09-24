# TPT CAD Format - build pipeline (Windows).
#
# Free edition (default):  ./build.ps1
#   cargo build --release --target wasm32-unknown-unknown (no pro feature)
#   wasm-bindgen --target web  ->  web repo public/apps/cad-format/ when
#   TPT_WEB_REPO is set (glue named cad-format.js, wasm keeps its _bg.wasm
#   name - the glue resolves it via import.meta.url), otherwise dist\web.
# Pro desktop edition:     ./build.ps1 -Pro
#   trunk build --release --features pro (its own independent wasm build,
#   into dist\ - NOT the wasm-bindgen path above) + cargo build the native
#   webview exe into release\. Deliberately does not touch
#   public/apps/cad-format/: that is the free in-browser demo's bundle, and
#   it must never be built with --features pro, or the site would give away
#   every paid feature (batch conversion, detailed reports, folder-watch)
#   for free. The two editions are separate cargo builds writing to separate
#   directories on purpose - do not try to "reuse" one build for both.
#
# Env:
#   TPT_WEB_REPO  path to the tptsolutions.co.nz web repo; the app is copied
#                 to <repo>\public\apps\cad-format\ for the hub runner.
#                 Defaults to the known local checkout below if unset.

[CmdletBinding()]
param(
    # Build the Pro desktop bundle (--features pro) instead of the free one.
    # Never writes to the free edition's $Dest - see header comment.
    [switch]$Pro,
    # Free-edition only: skip the cargo build and re-run wasm-bindgen against
    # an existing release build.
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-Location -LiteralPath $PSScriptRoot

$Crate = "tpt-app-cad-format"
$Target = "wasm32-unknown-unknown"
$OutName = "cad-format"
$Slug = "cad-format"
$DefaultWebRepo = "D:\Programming\2 WIP\TPT Electrician\tpt-electrician-nz-2"

Write-Host "== TPT CAD Format build ==" -ForegroundColor Cyan

if ($Pro) {
    Write-Host " edition: pro (batch conversion + detailed reports + folder-watch)"
    Write-Host "-- trunk build --release --features pro (standalone bundle for the desktop shell)" -ForegroundColor Cyan
    trunk build --release --features pro
    if ($LASTEXITCODE -ne 0) { throw "trunk build failed" }

    Write-Host "-- cargo build --release -p tpt-cad-desktop (webview exe)" -ForegroundColor Cyan
    cargo build --release -p tpt-cad-desktop
    if ($LASTEXITCODE -ne 0) { throw "desktop build failed" }
    New-Item -ItemType Directory -Force -Path (Join-Path $PSScriptRoot "release") | Out-Null
    Copy-Item "target\release\tpt-cad-format-pro.exe" "release\tpt-cad-format-pro.exe" -Force
    Write-Host "   exe: release\tpt-cad-format-pro.exe (serves dist\)" -ForegroundColor Cyan
    Write-Host "-- done" -ForegroundColor Green
    Write-Host "   (public/apps/cad-format/ was not touched - run ./build.ps1 with no flags to refresh the free demo)"
    exit 0
}

# Where the browser-loadable free bundle goes.
$WebRepo = if ($env:TPT_WEB_REPO) { $env:TPT_WEB_REPO } elseif (Test-Path $DefaultWebRepo) { $DefaultWebRepo } else { $null }
$Dest = if ($WebRepo) {
    Join-Path $WebRepo "public\apps\cad-format"
} else {
    Join-Path $PSScriptRoot "dist\web"
}

Write-Host " edition: free (single-file validation, no conversion)"
Write-Host " dest:    $Dest"

if (-not $SkipBuild) {
    Write-Host "-- cargo build --release --target $Target" -ForegroundColor Cyan
    cargo build --release --target $Target -p $Crate
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$Wasm = Join-Path $PSScriptRoot "target\$Target\release\tpt_app_cad_format.wasm"
if (-not (Test-Path $Wasm)) { throw "missing $Wasm - run the cargo build step first" }

Write-Host "-- wasm-bindgen --target web --out-name $OutName" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
wasm-bindgen --target web --out-name $OutName --out-dir $Dest $Wasm
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen failed" }

$Glue = Join-Path $Dest "$OutName.js"
$BgWasm = Join-Path $Dest "$($OutName)_bg.wasm"
foreach ($artifact in @($Glue, $BgWasm)) {
    if (-not (Test-Path $artifact)) { throw "wasm-bindgen did not produce $artifact" }
}

# Payload size report (todo Phase 4: budget ~2 MB gzipped; STEP parser included).
$raw = (Get-Item $BgWasm).Length
Add-Type -AssemblyName System.IO.Compression
$rawBytes = [System.IO.File]::ReadAllBytes($BgWasm)
$memoryStream = [System.IO.MemoryStream]::new()
$gzipStream = [System.IO.Compression.GZipStream]::new($memoryStream, [System.IO.Compression.CompressionLevel]::Optimal)
$gzipStream.Write($rawBytes, 0, $rawBytes.Length)
$gzipStream.Dispose()
$gzipped = $memoryStream.ToArray().Length
$memoryStream.Dispose()
$gzKb = [math]::Round($gzipped / 1KB, 1)
Write-Host "-- $(Split-Path $BgWasm -Leaf): $raw bytes raw, $gzipped bytes gzipped ($gzKb KB gz)" -ForegroundColor Cyan
if ($gzipped -gt 2MB) {
    Write-Warning "payload exceeds the ~2 MB gzip budget for the hub build"
}

Write-Host "-- done" -ForegroundColor Green
if (-not $WebRepo) {
    Write-Host "   (set TPT_WEB_REPO to copy the bundle into the hub web repo)"
} else {
    Write-Host ""
    Write-Host "Paste into src/lib/tpt-apps-registry.ts (TPT_APPS_REGISTRY), then flip status to 'live':" -ForegroundColor Cyan
    Write-Host @"
  {
    slug: '$Slug',
    name: 'TPT CAD Format',
    description: 'Validate and convert STEP/IGES CAD exchange files before they cause problems downstream.',
    category: 'engineering',
    status: 'coming-soon',
    wasm: { entry: '/apps/$Slug/$OutName.js' },
    // gumroadUrl: 'https://gum.co/...',  // add once the Pro exe is listed ($99)
  },
"@
}
