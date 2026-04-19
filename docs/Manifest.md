# Manifest and State File Reference

## Manifest format (`edgetx.yml`)

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
```

- `depends` references entries in `libraries`
- `exclude` takes glob patterns to skip during copy (e.g., `["*.luac", "presets.txt"]`)
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

## State file

Installed packages are tracked in `RADIO/packages.yml` on the SD card. The file is keyed by `id` — every installed package is uniquely identified by its canonical id.

```yaml
packages:
  # 1. Plain remote install
  - id: github.com/ExpressLRS/Lua-Scripts
    name: "ExpressLRS"
    channel: tag
    version: v1.6.0
    commit: abc123def456789...
    paths:
      - SCRIPTS/TOOLS/ELRS
      - SCRIPTS/ELRS

  # 2. Remote install with variant auto-selected for this radio
  - id: github.com/yaapu/FrskyTelemetryScript
    name: "Yaapu Telemetry"
    channel: tag
    version: v2.0.0
    commit: def456abc789012...
    variant: edgetx.color.yml
    paths:
      - WIDGETS/yaapu

  # 3. Subpackage (full-path id including subpackage segment)
  - id: github.com/offer-shmuely/lua-scripts/log-viewer
    channel: tag
    version: v1.0.0
    commit: 789abc012345678...
    paths:
      - SCRIPTS/TOOLS/LogViewer

  # 4. Local install (dev workflow)
  - id: github.com/me/my-widget
    name: "My Widget"
    channel: local
    local_path: /home/me/my-widget
    dev: true
    paths:
      - SCRIPTS/TOOLS/MyWidget

  # 5. Fork install (identity differs from fetch URL)
  - id: github.com/yaapu/FrskyTelemetryScript
    origin: github.com/me/FrskyTelemetryScript-fork
    channel: branch
    version: fix-branch
    commit: fed321...
    paths:
      - SCRIPTS/TELEMETRY/yaapu
```

### Field reference

| Field | Required | Description |
|---|---|---|
| `id` | yes | Canonical package identity (matches manifest `package.id`) |
| `name` | no | Human-friendly display name from manifest, omitted if empty |
| `channel` | yes | `tag`, `branch`, `commit`, or `local` — determines update semantics |
| `version` | no | Tag or branch name; empty for `commit`/`local` channels |
| `commit` | no | Full git SHA; empty for `local` channel |
| `origin` | no | Fetch URL when different from `id` (fork case) |
| `variant` | no | Variant manifest filename if one was selected (e.g. `edgetx.color.yml`) |
| `local_path` | no | Absolute filesystem path for `channel: local` installs |
| `paths` | yes | Content item root paths under the SD root — used for conflict detection |
| `dev` | no | `true` if `--dev` dependencies were included; persisted preference for updates |

### Channels

- `tag` — pinned to a semver release. `pkg update` fetches the newest semver tag matching the manifest.
- `branch` — follows branch HEAD. `pkg update` fetches the latest commit on the same branch.
- `commit` — pinned SHA. `pkg update` is a no-op unless the user overrides with `@<newref>`.
- `local` — installed from a filesystem directory. `pkg update` re-copies from `local_path`.

The `dev` field records whether development dependencies were included at install time. When running `pkg update` without `--dev`, the stored preference is preserved.

### Lookup semantics

The store indexes by `id`. CLI queries for `update`, `remove`, `info-on-installed` accept:

- Full canonical id: `github.com/ExpressLRS/Lua-Scripts`
- GitHub shorthand: `ExpressLRS/Lua-Scripts` (the `github.com/` host is assumed and filled in)
- GitHub shorthand with subpath: `ExpressLRS/Lua-Scripts/subpackage-name`

## File list files

Per-package file-level tracking lives under `RADIO/packages/` in a directory tree mirroring the package `id`. Each package gets its own directory containing a `files.list` CSV:

```
RADIO/packages/
├── github.com/
│   ├── ExpressLRS/
│   │   └── Lua-Scripts/
│   │       └── files.list
│   └── Org/
│       ├── Tools/
│       │   └── widget-a/
│       │       └── files.list
│       └── Repo/
│           └── files.list           # variant install (id = base, no extra segment)
```

This layout mirrors the fetch cache (`~/.cache/edgetx-cli/repos/{host}/{owner}/{repo}/...`) so there is a single mental model for "where stuff for a package lives." Each per-package directory is also a natural home for future per-package metadata (manifest snapshots, integrity checksums, install logs).

Each row in `files.list` contains a single path relative to the SD root. A trailing `/` marks a directory entry (used for bottom-up cleanup on uninstall).

Example `files.list` content:

```
SCRIPTS/TOOLS/ELRS/main.lua
SCRIPTS/TOOLS/ELRS/crossfire.lua
SCRIPTS/TOOLS/ELRS/
SCRIPTS/ELRS/lib.lua
SCRIPTS/ELRS/
```

These lists are written by `pkg install` / `pkg update` and consumed by `pkg remove` for precise file-level deletion, including `.luac` companions for any `.lua` file. On uninstall, the per-package directory and any now-empty parent directories are pruned up to `RADIO/packages/`.
