use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::manifest::{ContentItem, Manifest};
use crate::radio;

/// Event describes a single file change that was synced.
pub struct SyncEvent {
    pub op: String,
    pub rel_path: String,
}

/// Options for initial sync.
pub struct SyncOptions<'a> {
    pub manifest: &'a Manifest,
    pub manifest_dir: &'a Path,
    pub target_dir: &'a Path,
    pub items: &'a [ContentItem],
    pub on_initial_copy_start: Option<&'a dyn Fn(usize)>,
    pub on_file_copied: Option<&'a dyn Fn(&str)>,
}

/// Options for watch phase.
pub struct WatchOptions<'a> {
    pub manifest: &'a Manifest,
    pub manifest_dir: &'a Path,
    pub target_dir: &'a Path,
    pub items: &'a [ContentItem],
    pub on_sync_event: Option<&'a dyn Fn(SyncEvent)>,
    pub on_error: Option<&'a dyn Fn(&str)>,
}

/// Perform a full copy of all manifest items from source to target.
pub fn initial_sync(opts: SyncOptions) -> Result<usize> {
    let exclude_default: Vec<String> =
        radio::copy::DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect();

    // Count total files
    let mut total_files = 0;
    for item in opts.items {
        if let Ok(source_root) = opts.manifest.resolve_content_path(opts.manifest_dir, &item.path)
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
            &[item.path.as_str()],
            &radio::copy::CopyOptions {
                dry_run: false,
                exclude: &exclude,
                on_file: opts.on_file_copied.map(|cb| {
                    &*Box::leak(Box::new(move |dest: &Path| {
                        if let Ok(rel) = dest.strip_prefix(opts.target_dir) {
                            cb(&rel.to_string_lossy());
                        }
                    })) as &dyn Fn(&Path)
                }),
            },
        )?;
        total_copied += n;
    }

    Ok(total_copied)
}

/// Watch source directories for changes and sync them to target.
/// Blocks until Ctrl+C (SIGINT).
pub fn watch(opts: WatchOptions) -> Result<()> {
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })?;

    // Add watch dirs recursively
    for item in opts.items {
        if let Ok(source_root) = opts.manifest.resolve_content_path(opts.manifest_dir, &item.path)
        {
            let root = source_root.join(&item.path);
            if root.is_dir() {
                watcher.watch(&root, RecursiveMode::Recursive)?;
            }
        }
    }

    let debounce = Duration::from_millis(50);

    loop {
        // Collect events with debouncing
        let mut pending: HashMap<PathBuf, notify::EventKind> = HashMap::new();

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => {
                for path in event.paths {
                    pending.insert(path, event.kind);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Drain pending events with debounce
        std::thread::sleep(debounce);
        while let Ok(event) = rx.try_recv() {
            for path in event.paths {
                pending.insert(path, event.kind);
            }
        }

        // Process events
        for (path, kind) in &pending {
            process_event(path, kind, &opts);
        }
    }

    Ok(())
}

fn process_event(path: &Path, kind: &notify::EventKind, opts: &WatchOptions) {
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

    use notify::EventKind;
    match kind {
        EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
            let dest_path = opts.target_dir.join(&rel_path);
            let _ = std::fs::remove_file(&dest_path);
            if let Some(cb) = opts.on_sync_event {
                cb(SyncEvent {
                    op: "remove".into(),
                    rel_path,
                });
            }
        }
        EventKind::Create(_) | EventKind::Modify(_) => {
            if path.is_dir() {
                return;
            }

            let exclude = merge_default_exclude(&item.exclude);
            if radio::copy::is_excluded(path, &exclude) {
                return;
            }

            let dest_path = opts.target_dir.join(&rel_path);
            if let Some(parent) = dest_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let _ = radio::copy::copy_paths(
                &matched_root,
                opts.target_dir,
                &[rel_path.as_str()],
                &radio::copy::CopyOptions {
                    dry_run: false,
                    exclude: &exclude,
                    on_file: None,
                },
            );

            if let Some(cb) = opts.on_sync_event {
                cb(SyncEvent {
                    op: "copy".into(),
                    rel_path,
                });
            }
        }
        _ => {}
    }
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
        let item_path = &item.path;
        if rel_path.starts_with(item_path)
            && (rel_path.len() == item_path.len()
                || rel_path.as_bytes().get(item_path.len()) == Some(&b'/'))
        {
            return Some(item);
        }
    }
    None
}
