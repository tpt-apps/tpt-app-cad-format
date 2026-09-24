# Thin wrappers over build.ps1 (the canonical pipeline).
set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]

build:
    pwsh -NoLogo -NoProfile -File build.ps1

pro:
    pwsh -NoLogo -NoProfile -File build.ps1 -Pro

bindgen:
    pwsh -NoLogo -NoProfile -File build.ps1 -SkipBuild

test:
    cargo test -p tpt-cad-engine

check:
    cargo check --target wasm32-unknown-unknown -p tpt-app-cad-format
    cargo check --target wasm32-unknown-unknown -p tpt-app-cad-format --features pro

dev:
    trunk serve
