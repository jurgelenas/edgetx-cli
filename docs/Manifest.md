# Manifest Reference (`edgetx.yml`)

The `edgetx.yml` file describes a package and its contents. It is authored by the package maintainer and lives in the package repository.

For the runtime state files written to the SD card by `pkg install`/`update`/`remove`, see [State Files Reference](./State.md).

## Manifest format

The `edgetx.yml` file describes your package and its contents:

```yaml
package:
  id: github.com/ExpressLRS/Lua-Scripts                       # required: canonical URL-like path
  name: "ExpressLRS"                                          # optional: human-friendly display name
  description: ExpressLRS Lua scripts and widgets for EdgeTX  # required
  authors:                                                    # optional: list of authors
    - name: ExpressLRS Team
      email: info@expresslrs.org
  urls:                                                       # optional: project URLs
    - name: Homepage
      url: "https://www.expresslrs.org"
    - name: Repository
      url: "https://github.com/ExpressLRS/Lua-Scripts"
  screenshots:                                                # optional: relative paths to image files
    - assets/screen1.png
  keywords: ["telemetry", "elrs", "crossfire"]                # optional: keywords for discovery
  license: GPL-3.0-only                                       # optional: SPDX license expression
  source_dir: src                                             # optional: subdirectory containing source files
  min_edgetx_version: "2.12.0"                                # optional: minimum EdgeTX version required
  # binary: true                                              # optional: set to true for .luac bytecode

libraries:
  - name: ELRS
    path: SCRIPTS/ELRS
  - name: TestUtils
    path: SCRIPTS/TestUtils
    dev: true

tools:
  - name: ExpressLRS
    path: SCRIPTS/TOOLS/ExpressLRS
    depends:
      - ELRS
  - name: DebugTool
    path: SCRIPTS/TOOLS/DebugTool
    dev: true

widgets:
  - name: ELRSTelemetry
    path: WIDGETS/ELRSTelemetry
    depends:
      - ELRS
    exclude:
      - "*.luac"

telemetry:
  - name: MyTelem
    path: SCRIPTS/TELEMETRY/MyTelem

functions:
  - name: MyFunc
    path: SCRIPTS/FUNCTIONS/MyFunc

mixes:
  - name: MyMix
    path: SCRIPTS/MIXES/MyMix

sounds:
  - name: sounds-en
    path: SOUNDS/en

themes:
  - name: MyTheme
    path: THEMES/MyTheme
```

- `depends` references entries in `libraries`
- `exclude` takes glob patterns to skip during copy (e.g., `["*.luac", "presets.txt"]`)
- `themes` installs to `THEMES/<name>/` on the SD card — typically a directory with `theme.yml`, `logo.png`, and resolution-specific backgrounds. Themes require a color LCD; set `package.capabilities.display.type: colorlcd`
- `source_dir` is relative to the manifest file; all `path` values are relative to the source root and must use `/` as the separator (never `\`), since they represent paths on a FAT32 SD card
- `binary: true` disables the default `*.luac` exclusion, allowing compiled bytecode to be installed
- `dev: true` marks a content item as a development dependency - it is excluded from `pkg install` and `pkg update` unless `--dev` is passed, but included by default in `dev sync` (use `--no-dev` to exclude). A non-dev item cannot depend on a dev library

### Package fields

| Field | Required | Description |
|---|---|---|
| `id` | **yes** | Where the package lives: the git repo URL without the scheme (e.g. `github.com/ExpressLRS/Lua-Scripts`). See [Package id](#package-id) below. |
| `name` | no | Human-friendly display name (may contain spaces, punctuation, etc.). Falls back to the full `id` if absent. |
| `description` | **yes** | Non-empty description of the package. |
| `authors` | no | Array of `{name, email?}` objects. |
| `urls` | no | Array of `{name, url}` objects for project links. |
| `screenshots` | no | Array of relative paths to image files. Files must exist relative to the manifest directory. |
| `keywords` | no | Array of keyword strings for discovery. |
| `license` | no | SPDX license expression. Supports compound expressions (e.g., `"MIT OR Apache-2.0"`). |
| `source_dir` | no | Subdirectory containing source files, relative to the manifest. |
| `min_edgetx_version` | no | Minimum EdgeTX version required. |
| `binary` | no | Set to `true` to allow `.luac` bytecode installation. |

### Package id

`id` is the git repo location — the clone URL minus the scheme and `.git` suffix. It's how the package is uniquely identified and where it can be fetched from. Examples:

| Repository layout | Example id |
|---|---|
| Single-package repo on GitHub | `github.com/ExpressLRS/Lua-Scripts` |
| Subpackage in a multi-package repo | `github.com/offer-shmuely/lua-scripts/log-viewer` (clone URL: `github.com/offer-shmuely/lua-scripts`, package lives in `log-viewer/`) |
| Self-hosted Gitea / GitLab | `gitea.example.com/Team/widget-pack` |

The CLI accepts GitHub shorthand (`ExpressLRS/Lua-Scripts`) as input — it's expanded to `github.com/ExpressLRS/Lua-Scripts` automatically — but the manifest itself must declare the full form including host.

### Validation rules

- **`id`** must have at least three `/`-separated segments (`host/owner/repo`). The first segment must contain `.` (is a host). Each segment must match `^[a-zA-Z0-9][a-zA-Z0-9_.-]*$`. Extra segments beyond `host/owner/repo` are treated as a subpackage path inside the repo.
- **`description`** must be present and non-empty.
- **`license`** is validated as an SPDX expression. Compound expressions like `"MIT OR Apache-2.0"` and `"Apache-2.0 AND MIT"` are accepted.
- **`authors[].email`** is validated as an RFC 5321 email address when provided.
- **`urls[].url`** is validated as a well-formed URL.
- **`screenshots`** entries must point to files that exist relative to the manifest directory.

## Radio capabilities

Packages can declare what radio hardware they require using the `capabilities` field:

```yaml
package:
  id: github.com/me/my-widget
  capabilities:
    display:
      type: colorlcd          # "bw" or "colorlcd"
      resolution: 480x272     # optional: exact resolution (e.g., "128x64", "480x272")
      touch: true             # optional: requires touchscreen
```

All fields inside `display` are optional — omit any to mean "any". For example, `type: colorlcd` alone matches any color LCD radio regardless of resolution.

When installing a package, the CLI reads the radio's board info from the SD card and checks it against the catalog to determine display type, resolution, and other capabilities. If the package's filter doesn't match, a warning is shown.

## Variants

Variants are for the **same logical package** built for different radio hardware (BW vs color LCD, different resolutions). All variants share the same `id` — the hardware build is an install-time choice, not an identity distinction.

```yaml
# edgetx.yml (base manifest)
package:
  id: github.com/yaapu/FrskyTelemetryScript
  description: Yaapu Telemetry Script and Widget
  license: GPL-3.0-only
  source_dir: OTX_ETX
  min_edgetx_version: "2.11.0"
  variants:
    - path: edgetx.bw128x64.yml
      capabilities:
        display:
          type: bw
          resolution: 128x64
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
    - path: edgetx.color-touch.yml
      capabilities:
        display:
          type: colorlcd
          touch: true
```

**Variant files don't declare their own `id`** — they inherit from the base manifest. A variant file only lists its content (tools, widgets, etc.):

```yaml
# edgetx.color.yml — no `id` needed
package:
  description: Yaapu Telemetry (Color LCD)
widgets:
  - name: yaapu
    path: WIDGETS/yaapu
```

When `variants` is present, the CLI auto-selects the best matching variant based on the connected radio:

1. Loads the base manifest and sees `variants`
2. Detects radio capabilities from the SD card (board → catalog lookup)
3. Matches capabilities against each variant's filter (most specific match wins)
4. Loads the selected variant's content

**Manual variant selection:** two equivalent forms override auto-selection.

```
pkg install yaapu/FrskyTelemetryScript --path edgetx.bw128x64.yml
pkg install yaapu/FrskyTelemetryScript::edgetx.bw128x64.yml
```

**Update behavior:** `pkg update` always keeps the currently-installed variant. To switch variants, use `pkg install` explicitly (with or without the override).

Variant resolution is one level deep — a variant manifest should not itself declare further variants.

## Subpackages

Subpackages are **distinct packages living in the same repository**, each with its own manifest and its own `id` that includes the subpackage path.

Example: a repo `github.com/offer-shmuely/lua-scripts` containing multiple independent tools, each in its own subdirectory with its own `edgetx.yml`. A subpackage can itself declare hardware variants — the variant files live as siblings of its `edgetx.yml` inside the subpackage directory:

```
offer-shmuely/lua-scripts/
├── log-viewer/
│   ├── edgetx.yml                    # base (declares variants)
│   ├── edgetx.bw128x64.yml           # variant
│   └── edgetx.color.yml              # variant
└── cell-mix/
    └── edgetx.yml
```

```yaml
# log-viewer/edgetx.yml — base, declares variants
package:
  id: github.com/offer-shmuely/lua-scripts/log-viewer
  description: Flight log viewer
  license: GPL-3.0-only
  min_edgetx_version: "2.11.0"
  variants:
    - path: edgetx.bw128x64.yml
      capabilities:
        display:
          type: bw
          resolution: 128x64
    - path: edgetx.color.yml
      capabilities:
        display:
          type: colorlcd
```

```yaml
# log-viewer/edgetx.color.yml — variant content, inherits id
package:
  description: Flight log viewer (color)

tools:
  - name: LogViewer
    path: SCRIPTS/TOOLS/LogViewer
```

```yaml
# cell-mix/edgetx.yml — single package, no variants
package:
  id: github.com/offer-shmuely/lua-scripts/cell-mix
  description: Battery cell voltage divider mix script
  license: GPL-3.0-only

mixes:
  - name: cell
    path: SCRIPTS/MIXES/cell.lua
```

Subpackages are independent — they can be installed/updated/removed separately and pinned to different versions:

```
pkg install offer-shmuely/lua-scripts/log-viewer@v1.0.0
pkg install offer-shmuely/lua-scripts/cell-mix@v2.0.0
```

The CLI parses 4+ path segments as `host/owner/repo/subpath`. The clone URL is `https://host/owner/repo.git`; the manifest is loaded from `subpath/edgetx.yml`.

### Flat-file layout

When subpackages share a source tree and moving files into per-package subdirectories would be invasive, manifests can instead sit as flat siblings at the repo root. The file name encodes the subpackage:

```
offer-shmuely/lua-scripts/
├── edgetx.log-viewer.yml
├── edgetx.cell-mix.yml
└── SCRIPTS/
    ├── TOOLS/LogViewer/
    └── MIXES/cell.lua
```

Each flat manifest still declares its own full-path `id` (e.g. `github.com/offer-shmuely/lua-scripts/log-viewer`), so identity is unchanged — only the on-disk layout differs.

**Resolution order** for `pkg install owner/repo/<sub>`:

1. `<sub>/edgetx.yml` (subdirectory form — preferred when present)
2. `edgetx.<sub>.yml` (flat-file fallback)

Multi-segment subpaths map to dotted names: `a/b` → `a/b/edgetx.yml`, falling back to `edgetx.a.b.yml`.

The install command is identical for both layouts — `--path` is not required in either case.
