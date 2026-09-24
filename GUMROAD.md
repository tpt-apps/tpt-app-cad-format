# Gumroad listing

Everything needed to create/update the Gumroad product once this app is
built. Source copy is `apps.md` §T1-14 in the `tpt-electrician-nz-2` repo — if
that changes, update here too (and vice versa).

## Step-by-step: publish this listing

1. Build the Pro edition (`./build.ps1 -Pro`, once the build pipeline exists
   — see `todo.md` and mirror `tpt-app-fea-lite`/`tpt-app-cutlist-optimizer`'s
   pattern) — produces the packaged exe/zip in `release/`.
2. Log in to Gumroad → **New product** → **Digital product**.
3. Set the product name, price, and upload the packaged file (all below).
4. Paste the short description, bullets, and full description (below) into
   their respective fields.
5. Set category/tags (below) and add a cover image (see note below).
6. Publish, then copy the resulting product URL (`https://gum.co/...`).
7. Paste that URL into the `cad-format` entry's `gumroadUrl` field in
   `tpt-electrician-nz-2\src\lib\tpt-apps-registry.ts`, and set `price: '$99'`
   on the same entry.
8. Flip that entry's `status` to `'live'` once both the free demo and this
   Gumroad listing exist.
9. Commit and push the registry repo.

## File to upload

Not built yet. Once packaged, this will be the Pro exe/zip produced by the
app's `build.ps1 -Pro` (or equivalent) — see `todo.md`.

## Product name

```
TPT CAD Format — STEP/IGES Validation & Conversion
```

## Price

**$99 USD, one-time purchase** (no subscription).

## Short description / summary field

```
Validate and convert industrial CAD exchange files before they cause problems downstream — built for manufacturing engineers moving data between legacy systems.
```

## Bullets / feature list

```
STEP/IGES validation
Batch format conversion
Detailed geometry-issue reports
Folder-watch automation
One-time purchase
```

## Full description (long-form field)

```
TPT CAD Format validates and converts industrial CAD/data-exchange file
formats, flagging geometry issues before they cause downstream problems in a
customer's or supplier's CAD system.

Who it's for: manufacturing engineers exchanging CAD data (STEP/IGES)
between legacy systems who need quick format conversion/validation without a
full CAD-interoperability suite subscription.

Pro edition (this download):
- Batch conversion between formats
- Detailed geometry-issue reports
- Folder-watch automation

Try the free single-file validation edition first, right in your browser, no
install: tptsolutions.co.nz/tools/cad-format

Requirements: Windows 10 (20H2+) or Windows 11. Uses the WebView2 runtime,
preinstalled on both — nothing extra to download.
```

## Category / tags

- Category: **Software > Utility** (or Gumroad's closest equivalent)
- Tags: `cad`, `step-file`, `iges`, `manufacturing-engineering`,
  `data-conversion`, `windows`

## Cover image / thumbnail

Not created yet. A screenshot of the geometry-issue report flagging a real
problem is the most convincing cover once built. No stock/AI art.

## After publishing

1. Copy the resulting product URL (`https://gum.co/...`).
2. Paste it into the `gumroadUrl` field of the `cad-format` entry in
   `tpt-electrician-nz-2\src\lib\tpt-apps-registry.ts`, and set `price: '$99'`.
3. Set `status` to `'live'`.
4. Commit and push the registry repo.
