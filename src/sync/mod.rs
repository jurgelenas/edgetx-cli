use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use thiserror::Error;

use crate::manifest::{ContentItem, Manifest};
use crate::radio;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error(transparent)]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("file watcher: {source}")]
    Watcher { source: notify::Error },
    #[error(transparent)]
    Copy(#[from] crate::radio::copy::CopyError),
}

/// Event describes a single file change that was synced.
pub struct SyncEvent {
    pub op: String,
    pub rel_path: String,
}

pub type OnInitialCopyStartFn<'a> = &'a dyn Fn(usize);
pub type OnFileCopiedFn<'a> = &'a dyn Fn(&str);
pub type OnSyncEventFn<'a> = &'a dyn Fn(SyncEvent);
pub type OnErrorFn<'a> = &'a dyn Fn(&str);

/// Options for initial sync.
pub struct SyncOptions<'a> {
    pub manifest: &'a Manifest,
    pub manifest_dir: &'a Path,
    pub target_dir: &'a Path,
    pub items: &'a [ContentItem],
    pub on_initial_copy_start: Option<OnInitialCopyStartFn<'a>>,
    pub on_file_copied: Option<OnFileCopiedFn<'a>>,
}

/// Options for watch phase.
pub struct WatchOptions<'a> {
    pub manifest: &'a Manifest,
    pub manifest_dir: &'a Path,
    pub target_dir: &'a Path,
    pub items: &'a [ContentItem],
    pub on_sync_event: Option<OnSyncEventFn<'a>>,
    #[allow(dead_code)]
    pub on_error: Option<OnErrorFn<'a>>,
}

/// Perform a full copy of all manifest items from source to target.
pub fn initial_sync(opts: SyncOptions) -> Result<usize, SyncError> {
    let _exclude_default: Vec<String> = radio::copy::DEFAULT_EXCLUDE
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Count total files
    let mut total_files = 0;
    for item in opts.items {
        if let Ok(source_root) = opts
            .manifest
            .resolve_content_path(opts.manifest_dir, &item.path)
        {
            let exclude = merge_default_exclude(&item.exclude);
            total_files += radio::copy::count_files(&source_root, &[item.path.as_str()], &exclude);
        }
    }

    if let Some(cb) = opts.on_initial_copy_start {
        cb(total_files);
    }

    let mut total_copied = 0;
    for item in opts.items {
        let source_root = opts
            .manifest
            .resolve_content_path(opts.manifest_dir, &item.path)?;

        let exclude = merge_default_exclude(&item.exclude);
        let n = radio::copy::copy_paths(
            &source_root,
            opts.target_dir,
            &[radio::copy::CopyPath {
                src: item.path.as_str(),
                dest: item.sd_dest().as_str(),
            }],
            &radio::copy::CopyOptions {
                dry_run: false,
                exclude: &exclude,
            },
            &mut |dest: &Path| {
                if let Ok(rel) = dest.strip_prefix(opts.target_dir)
                    && let Some(cb) = opts.on_file_copied
                {
                    cb(&rel.to_string_lossy());
                }
            },
        )?;
        total_copied += n;
    }

    Ok(total_copied)
}

/// Watch source directories for changes and sync them to target.
/// Blocks until Ctrl+C (SIGINT).
pub fn watch(opts: WatchOptions) -> Result<(), SyncError> {
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .map_err(|e| SyncError::Watcher { source: e })?;

    // Add watch dirs recursively
    for item in opts.items {
        if let Ok(source_root) = opts
            .manifest
            .resolve_content_path(opts.manifest_dir, &item.path)
        {
            let root = source_root.join(item.path.as_str());
            if root.is_dir() {
                watcher
                    .watch(&root, RecursiveMode::Recursive)
                    .map_err(|e| SyncError::Watcher { source: e })?;
            }
        }
    }

    let debounce = Duration::from_millis(50);

    loop {
        // Collect events with debouncing. We track, per path, whether *any*
        // actionable event was seen — OR-combining so a trailing pure-read event
        // (e.g. the IN_CLOSE_WRITE / IN_OPEN that ends an editor's save) cannot
        // erase an earlier Create/Modify for the same path. Storing only the
        // last event kind would let that trailing read mask a real change.
        let mut pending: HashMap<PathBuf, bool> = HashMap::new();

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => merge_event(&mut pending, &event),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Drain pending events with debounce
        std::thread::sleep(debounce);
        while let Ok(event) = rx.try_recv() {
            merge_event(&mut pending, &event);
        }

        // Process events whose collected signal is actionable.
        for (path, actionable) in &pending {
            if *actionable {
                process_event(path, &opts);
            }
        }
    }

    Ok(())
}

/// Fold an event into the pending map, marking each touched path actionable if
/// the event represents a change (anything other than a pure read).
fn merge_event(pending: &mut HashMap<PathBuf, bool>, event: &notify::Event) {
    let actionable = is_actionable(&event.kind);
    for path in &event.paths {
        let entry = pending.entry(path.clone()).or_insert(false);
        *entry |= actionable;
    }
}

/// Whether an event signals a change that should trigger a re-sync. Pure read
/// access (open/read/close-after-read) is ignored so merely opening or reading a
/// source file does not re-copy it; a write-close (IN_CLOSE_WRITE) does count.
fn is_actionable(kind: &notify::EventKind) -> bool {
    use notify::EventKind;
    use notify::event::{AccessKind, AccessMode};
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

fn process_event(path: &Path, opts: &WatchOptions) {
    // Find which source root this path belongs to
    let mut rel_path = None;
    let mut matched_root = None;

    for root in opts.manifest.source_roots(opts.manifest_dir) {
        if let Ok(rel) = path.strip_prefix(&root) {
            let rel_str = rel.to_string_lossy().to_string();
            if find_manifest_item(&rel_str, opts.items).is_some() {
                rel_path = Some(rel_str);
                matched_root = Some(root);
                break;
            }
        }
    }

    let (rel_path, matched_root) = match (rel_path, matched_root) {
        (Some(r), Some(m)) => (r, m),
        _ => return,
    };

    let item = match find_manifest_item(&rel_path, opts.items) {
        Some(item) => item,
        None => return,
    };

    // Gate excluded paths (e.g. *.luac) up front so they produce no filesystem
    // op or log line in either direction.
    let exclude = merge_default_exclude(&item.exclude);
    if radio::copy::is_excluded(path, &exclude) {
        return;
    }

    let dest_path = opts.target_dir.join(&rel_path);

    // Dispatch on the source's current on-disk state rather than the event kind.
    // notify's rename reporting is platform-dependent (From/To/Both), so the
    // settled disk state is the reliable signal: a path that now exists as a
    // file must be copied (create, in-place modify, or the rename-to half of an
    // atomic save); one that is gone must be removed (delete, or the rename-from
    // half — including editor temp files).
    if path.is_file() {
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let _ = radio::copy::copy_paths(
            &matched_root,
            opts.target_dir,
            &[radio::copy::CopyPath::same(rel_path.as_str())],
            &radio::copy::CopyOptions {
                dry_run: false,
                exclude: &exclude,
            },
            &mut |_| {},
        );

        if let Some(cb) = opts.on_sync_event {
            cb(SyncEvent {
                op: "copy".into(),
                rel_path,
            });
        }
    } else if !path.exists() {
        let _ = std::fs::remove_file(&dest_path);
        let _ = std::fs::remove_dir_all(&dest_path);
        if let Some(cb) = opts.on_sync_event {
            cb(SyncEvent {
                op: "remove".into(),
                rel_path,
            });
        }
    }
    // Otherwise the path is an existing directory — its contents arrive as their
    // own per-file events, so there is nothing to do here.
}

fn merge_default_exclude(extra: &[String]) -> Vec<String> {
    let mut exclude: Vec<String> = radio::copy::DEFAULT_EXCLUDE
        .iter()
        .map(|s| s.to_string())
        .collect();
    exclude.extend(extra.iter().cloned());
    exclude
}

fn find_manifest_item<'a>(rel_path: &str, items: &'a [ContentItem]) -> Option<&'a ContentItem> {
    for item in items {
        let item_path = item.path.as_str();
        if rel_path.starts_with(item_path)
            && (rel_path.len() == item_path.len()
                || rel_path.as_bytes().get(item_path.len()) == Some(&b'/'))
        {
            return Some(item);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ContentItem;
    use crate::packages::path::PackagePath;
    use notify::EventKind;
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, Event, ModifyKind, RemoveKind, RenameMode,
    };
    use std::cell::RefCell;
    use tempfile::TempDir;

    const REL: &str = "SCRIPTS/TOOLS/MyTool/main.lua";

    fn tool_item() -> ContentItem {
        ContentItem {
            name: "MyTool".into(),
            path: PackagePath::new("SCRIPTS/TOOLS/MyTool"),
            dest: None,
            depends: vec![],
            exclude: vec![],
            dev: false,
        }
    }

    /// Run `process_event` for `rel` under `src`/`target` and return the
    /// (op, rel_path) sync events that fired.
    fn run(src: &Path, target: &Path, rel: &str) -> Vec<(String, String)> {
        // Default manifest has an empty source_dir, so its source root is the
        // manifest dir itself (the source tempdir).
        let manifest = Manifest::default();
        let items = vec![tool_item()];
        let events: RefCell<Vec<(String, String)>> = RefCell::new(Vec::new());
        let on_sync_event = |e: SyncEvent| {
            events.borrow_mut().push((e.op, e.rel_path));
        };
        let opts = WatchOptions {
            manifest: &manifest,
            manifest_dir: src,
            target_dir: target,
            items: &items,
            on_sync_event: Some(&on_sync_event),
            on_error: None,
        };
        process_event(&src.join(rel), &opts);
        events.into_inner()
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn existing_file_is_copied() {
        let src = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        write(&src.path().join(REL), "-- updated");

        let events = run(src.path(), target.path(), REL);

        let dest = target.path().join(REL);
        assert!(dest.is_file(), "dest should be copied");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "-- updated");
        assert_eq!(events, vec![("copy".to_string(), REL.to_string())]);
    }

    #[test]
    fn missing_file_removes_dest() {
        let src = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        // Destination exists; source does not (it was deleted / renamed away).
        write(&target.path().join(REL), "-- old");

        let events = run(src.path(), target.path(), REL);

        assert!(!target.path().join(REL).exists());
        assert_eq!(events, vec![("remove".to_string(), REL.to_string())]);
    }

    #[test]
    fn excluded_luac_ignored() {
        let src = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let rel = "SCRIPTS/TOOLS/MyTool/main.luac";
        write(&src.path().join(rel), "bytecode");

        let events = run(src.path(), target.path(), rel);

        assert!(!target.path().join(rel).exists());
        assert!(events.is_empty());
    }

    #[test]
    fn pure_reads_are_not_actionable() {
        assert!(!is_actionable(&EventKind::Access(AccessKind::Read)));
        assert!(!is_actionable(&EventKind::Access(AccessKind::Open(
            AccessMode::Read
        ))));
        assert!(!is_actionable(&EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
    }

    #[test]
    fn changes_are_actionable() {
        assert!(is_actionable(&EventKind::Create(CreateKind::File)));
        assert!(is_actionable(&EventKind::Modify(ModifyKind::Data(
            DataChange::Content
        ))));
        assert!(is_actionable(&EventKind::Modify(ModifyKind::Name(
            RenameMode::To
        ))));
        assert!(is_actionable(&EventKind::Remove(RemoveKind::File)));
        // The write-close that ends an editor save must count as a change.
        assert!(is_actionable(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
    }

    fn event(kind: EventKind, path: &str) -> Event {
        Event {
            kind,
            paths: vec![PathBuf::from(path)],
            attrs: Default::default(),
        }
    }

    // Regression for the vim-save bug: a Modify followed (within one debounce
    // window) by the trailing IN_CLOSE_WRITE for the same path must stay
    // actionable. The old code stored only the last event kind, so the trailing
    // Access event masked the Modify and the file was never re-copied.
    #[test]
    fn trailing_write_close_keeps_path_actionable() {
        let mut pending: HashMap<PathBuf, bool> = HashMap::new();
        merge_event(
            &mut pending,
            &event(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                "/src/foo.lua",
            ),
        );
        merge_event(
            &mut pending,
            &event(
                EventKind::Access(AccessKind::Close(AccessMode::Write)),
                "/src/foo.lua",
            ),
        );
        assert_eq!(pending.get(Path::new("/src/foo.lua")), Some(&true));
    }

    // A pure read sequence (open + read + close) on a path that had no change
    // must remain non-actionable, so merely reading a source file does not
    // re-copy it.
    #[test]
    fn pure_read_sequence_stays_non_actionable() {
        let mut pending: HashMap<PathBuf, bool> = HashMap::new();
        for kind in [
            EventKind::Access(AccessKind::Open(AccessMode::Read)),
            EventKind::Access(AccessKind::Read),
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
        ] {
            merge_event(&mut pending, &event(kind, "/src/foo.lua"));
        }
        assert_eq!(pending.get(Path::new("/src/foo.lua")), Some(&false));
    }
}
