# State Files Reference

The CLI writes two kinds of state to the SD card to track what's installed:

- `RADIO/packages.yml` — one row per installed package (the install registry)
- `RADIO/packages/.../files.list` — per-package file-level inventory (for clean removal)

These files are generated and maintained by `pkg install` / `pkg update` / `pkg remove`. You should not edit them by hand.

## `packages.yml`

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

## `files.list`

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
