# Manifest and State File Reference

## Manifest format (`edgetx.yml`)

The `edgetx.yml` file describes your package and its contents:

```yaml
package:
  id: expresslrs                                   # required: machine identifier
  name: "ExpressLRS"                               # optional: human-friendly display name
  description: ExpressLRS Lua scripts and widgets for EdgeTX  # required
  authors:                                         # optional: list of authors
    - name: ExpressLRS Team
      email: info@expresslrs.org
  urls:                                            # optional: project URLs
    - name: Homepage
      url: "https://www.expresslrs.org"
    - name: Repository
      url: "https://github.com/ExpressLRS/ExpressLRS"
  screenshots:                                     # optional: relative paths to image files
    - assets/screen1.png
  keywords: ["telemetry", "elrs", "crossfire"]     # optional: keywords for discovery
  license: GPL-3.0                                 # optional: SPDX license expression
  source_dir: src                                  # optional: subdirectory containing source files
  min_edgetx_version: "2.12.0"                     # optional: minimum EdgeTX version required
  # binary: true                                   # optional: set to true for packages distributing .luac bytecode

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
```

- `depends` references entries in `libraries`
- `exclude` takes glob patterns to skip during copy (e.g., `["*.luac", "presets.txt"]`)
- `source_dir` is relative to the manifest file; all `path` values are relative to the source root and must use `/` as the separator (never `\`), since they represent paths on a FAT32 SD card
- `binary: true` disables the default `*.luac` exclusion, allowing compiled bytecode to be installed
- `dev: true` marks a content item as a development dependency - it is excluded from `pkg install` and `pkg update` unless `--dev` is passed, but included by default in `dev sync` (use `--no-dev` to exclude). A non-dev item cannot depend on a dev library

### Package fields

| Field | Required | Description |
|---|---|---|
| `id` | **yes** | Machine identifier. Must match `^[a-zA-Z0-9][a-zA-Z0-9_-]*$`. |
| `name` | no | Human-friendly display name (may contain spaces, punctuation, etc.). |
| `description` | **yes** | Non-empty description of the package. |
| `authors` | no | Array of `{name, email?}` objects. |
| `urls` | no | Array of `{name, url}` objects for project links. |
| `screenshots` | no | Array of relative paths to image files. Files must exist relative to the manifest directory. |
| `keywords` | no | Array of keyword strings for discovery. |
| `license` | no | SPDX license expression. Supports compound expressions (e.g., `"MIT OR Apache-2.0"`). |
| `source_dir` | no | Subdirectory containing source files, relative to the manifest. |
| `min_edgetx_version` | no | Minimum EdgeTX version required. |
| `binary` | no | Set to `true` to allow `.luac` bytecode installation. |

### Validation rules

- **`id`** must match the regex `^[a-zA-Z0-9][a-zA-Z0-9_-]*$` (starts with an alphanumeric character, followed by alphanumerics, hyphens, or underscores).
- **`description`** must be present and non-empty.
- **`license`** is validated as an SPDX expression. Compound expressions like `"MIT OR Apache-2.0"` and `"Apache-2.0 AND MIT"` are accepted.
- **`authors[].email`** is validated as an RFC 5321 email address when provided.
- **`urls[].url`** is validated as a well-formed URL.
- **`screenshots`** entries must point to files that exist relative to the manifest directory.

## Radio capabilities

Packages can declare what radio hardware they require using the `capabilities` field:

```yaml
package:
  id: my-widget
  capabilities:
    display:
      type: colorlcd          # "bw" or "colorlcd"
      resolution: 480x272     # optional: exact resolution (e.g., "128x64", "480x272")
      touch: true             # optional: requires touchscreen
```

All fields inside `display` are optional — omit any to mean "any". For example, `type: colorlcd` alone matches any color LCD radio regardless of resolution.

When installing a package, the CLI reads the radio's board info from the SD card and checks it against the catalog to determine display type, resolution, and other capabilities. If the package's filter doesn't match, a warning is shown.

## Variants

When a package supports multiple radio types, the manifest can declare **variants** — pointers to other manifest files, each targeting specific capabilities:

```yaml
# edgetx.yml
package:
  id: yaapu-telemetry
  description: Yaapu Telemetry Script and Widget
  license: GPL-3.0
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

When `variants` is present, the CLI auto-selects the best matching variant based on the connected radio:

1. Loads the manifest and sees `variants`
2. Detects radio capabilities from the SD card (board -> catalog lookup)
3. Matches capabilities against each variant's filter
4. Loads the selected variant manifest instead (most specific match wins)
5. If no radio is detected, falls back to the base manifest (use `--path` to select manually)

**`--path` always overrides auto-selection** — you can point directly at a specific variant manifest.

### Multi-package repositories

Any manifest file can declare variants, not just `edgetx.yml`. This supports repositories that contain multiple separate packages, each with their own variants:

```yaml
# edgetx.log-viewer.yml
package:
  id: log-viewer
  description: EdgeTX Log Viewer
  license: GPL-3.0
  min_edgetx_version: "2.11.0"
  variants:
    - path: edgetx.log-viewer.bw128x64.yml
      capabilities:
        display:
          type: bw
          resolution: 128x64
    - path: edgetx.log-viewer.color.yml
      capabilities:
        display:
          type: colorlcd
```

Variant resolution is one level deep — a variant manifest should not itself declare further variants.

## State file

Installed packages are tracked in `RADIO/packages.yml` on the SD card:

```yaml
packages:
  - source: ExpressLRS/Lua-Scripts
    id: expresslrs
    name: "ExpressLRS"
    channel: tag
    version: v1.6.0
    commit: abc123def456789...
    paths:
      - SCRIPTS/TOOLS/ELRS
      - SCRIPTS/ELRS

  - source: "Org/Repo::edgetx.c480x272.yml"
    id: yaapu-color
    channel: tag
    version: v2.0.0
    commit: def456abc789012...
    paths:
      - WIDGETS/Yaapu

  - source: "local::/home/user/my-project"
    id: my-tool
    channel: local
    dev: true
    paths:
      - SCRIPTS/TOOLS/MyTool
      - SCRIPTS/TOOLS/DebugTool
```

Channels: `tag` (semver release), `branch` (branch HEAD), `commit` (pinned SHA), `local` (local directory).

The `dev` field records whether development dependencies were included at install time. When running `pkg update` without `--dev`, the stored preference is preserved.

The optional `name` field stores the human-friendly display name from the manifest, used for user-facing output.

Individual file lists are stored as CSV in `RADIO/packages/<id>.list`. Each row contains a single file path relative to the SD root. These lists are used for precise file-level removal when uninstalling a package.
