# Manifest and State File Reference

## Manifest format (`edgetx.yml`)

The `edgetx.yml` file describes your package and its contents:

```yaml
package:
  name: expresslrs
  description: ExpressLRS Lua scripts and widgets for EdgeTX
  license: GPL-3.0        # optional: SPDX license identifier
  source_dir: src          # optional: subdirectory containing source files
  # binary: true           # optional: set to true for packages distributing .luac bytecode

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

## State file

Installed packages are tracked in `RADIO/packages.yml` on the SD card:

```yaml
packages:
  - source: ExpressLRS/Lua-Scripts
    name: expresslrs
    channel: tag
    version: v1.6.0
    commit: abc123def456789...
    paths:
      - SCRIPTS/TOOLS/ELRS
      - SCRIPTS/ELRS

  - source: "Org/Repo::edgetx.c480x272.yml"
    name: yaapu-color
    channel: tag
    version: v2.0.0
    commit: def456abc789012...
    paths:
      - WIDGETS/Yaapu

  - source: "local::/home/user/my-project"
    name: my-tool
    channel: local
    dev: true
    paths:
      - SCRIPTS/TOOLS/MyTool
      - SCRIPTS/TOOLS/DebugTool
```

Channels: `tag` (semver release), `branch` (branch HEAD), `commit` (pinned SHA), `local` (local directory).

The `dev` field records whether development dependencies were included at install time. When running `pkg update` without `--dev`, the stored preference is preserved.

Individual file lists are stored as CSV in `RADIO/packages/<name>.list`. Each row contains a single file path relative to the SD root. These lists are used for precise file-level removal when uninstalling a package.
