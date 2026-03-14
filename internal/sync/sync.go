package sync

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"time"


	"github.com/jurgelenas/edgetx-cli/pkg/logging"
	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/jurgelenas/edgetx-cli/internal/radio"
	"github.com/fsnotify/fsnotify"
)

// Event describes a single file change that was synced.
type Event struct {
	Op      string // "copy" or "remove"
	RelPath string // path relative to sourceRoot (e.g. "SCRIPTS/ELRS/crsf.lua")
}

// Callbacks allows the caller to hook into sync lifecycle events.
type Callbacks struct {
	OnInitialCopyStart func(totalFiles int)
	OnFileCopied       func(event Event)
	OnWatchReady       func()
	OnSyncEvent        func(event Event)
	OnError            func(err error)
}

// Options configures the sync operation.
type Options struct {
	Manifest    *manifest.Manifest
	ManifestDir string
	TargetDir   string
	Items       []manifest.ContentItem
	Callbacks   Callbacks
}

// InitialSync performs a full copy of all manifest items from source to target.
func InitialSync(opts Options) (int, error) {
	totalFiles := 0
	for _, item := range opts.Items {
		sourceRoot, err := opts.Manifest.ResolveContentPath(opts.ManifestDir, item.Path)
		if err != nil {
			continue
		}
		exclude := mergeDefaultExclude(item.Exclude)
		totalFiles += radio.CountFiles(sourceRoot, []string{item.Path}, exclude)
	}

	if opts.Callbacks.OnInitialCopyStart != nil {
		opts.Callbacks.OnInitialCopyStart(totalFiles)
	}

	totalCopied := 0
	for _, item := range opts.Items {
		sourceRoot, err := opts.Manifest.ResolveContentPath(opts.ManifestDir, item.Path)
		if err != nil {
			return totalCopied, err
		}
		copyOpts := radio.CopyOptions{
			Exclude: mergeDefaultExclude(item.Exclude),
			OnFile: func(dest string) {
				if opts.Callbacks.OnFileCopied != nil {
					relPath, _ := filepath.Rel(opts.TargetDir, dest)
					opts.Callbacks.OnFileCopied(Event{Op: "copy", RelPath: relPath})
				}
			},
		}
		n, err := radio.CopyPaths(sourceRoot, opts.TargetDir, []string{item.Path}, copyOpts)
		if err != nil {
			return totalCopied, err
		}
		totalCopied += n
	}

	return totalCopied, nil
}

// Watch starts watching all source directories for changes and syncs them
// to the target directory. It blocks until ctx is cancelled.
func Watch(ctx context.Context, opts Options) error {
	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		return err
	}
	defer watcher.Close()

	if err := addWatchDirsRecursive(watcher, opts); err != nil {
		return err
	}

	if opts.Callbacks.OnWatchReady != nil {
		opts.Callbacks.OnWatchReady()
	}

	const debounceInterval = 50 * time.Millisecond
	pending := make(map[string]fsnotify.Event)
	timer := time.NewTimer(debounceInterval)
	timer.Stop()

	for {
		select {
		case <-ctx.Done():
			return nil

		case fsEvent, ok := <-watcher.Events:
			if !ok {
				return nil
			}
			pending[fsEvent.Name] = fsEvent
			timer.Reset(debounceInterval)

		case err, ok := <-watcher.Errors:
			if !ok {
				return nil
			}
			if opts.Callbacks.OnError != nil {
				opts.Callbacks.OnError(err)
			}

		case <-timer.C:
			for path, fsEvent := range pending {
				processFSEvent(watcher, path, fsEvent, opts)
			}
			clear(pending)
		}
	}
}

func processFSEvent(watcher *fsnotify.Watcher, path string, fsEvent fsnotify.Event, opts Options) {
	// Try each source root to find which one this path belongs to.
	var relPath string
	var matchedRoot string
	for _, root := range opts.Manifest.SourceRoots(opts.ManifestDir) {
		rel, err := filepath.Rel(root, path)
		if err != nil || strings.HasPrefix(rel, "..") {
			continue
		}
		if findManifestItem(rel, opts.Items) != nil {
			relPath = rel
			matchedRoot = root
			break
		}
	}
	if matchedRoot == "" {
		return
	}

	item := findManifestItem(relPath, opts.Items)
	if item == nil {
		return
	}

	if fsEvent.Has(fsnotify.Remove) || fsEvent.Has(fsnotify.Rename) {
		destPath := filepath.Join(opts.TargetDir, relPath)
		if err := os.Remove(destPath); err != nil && !os.IsNotExist(err) {
			logging.WithField("path", relPath).Warn("failed to remove synced file")
		}
		if opts.Callbacks.OnSyncEvent != nil {
			opts.Callbacks.OnSyncEvent(Event{Op: "remove", RelPath: relPath})
		}
		return
	}

	if fsEvent.Has(fsnotify.Create) || fsEvent.Has(fsnotify.Write) {
		info, err := os.Stat(path)
		if err != nil {
			return
		}

		if info.IsDir() {
			watcher.Add(path)
			return
		}

		if radio.IsExcluded(filepath.Base(path), mergeDefaultExclude(item.Exclude)) {
			return
		}

		destPath := filepath.Join(opts.TargetDir, relPath)
		if err := os.MkdirAll(filepath.Dir(destPath), 0o755); err != nil {
			if opts.Callbacks.OnError != nil {
				opts.Callbacks.OnError(err)
			}
			return
		}

		copyOpts := radio.CopyOptions{}
		if _, err := radio.CopyPaths(matchedRoot, opts.TargetDir, []string{relPath}, copyOpts); err != nil {
			if opts.Callbacks.OnError != nil {
				opts.Callbacks.OnError(err)
			}
			return
		}

		if opts.Callbacks.OnSyncEvent != nil {
			opts.Callbacks.OnSyncEvent(Event{Op: "copy", RelPath: relPath})
		}
	}
}

func addWatchDirsRecursive(watcher *fsnotify.Watcher, opts Options) error {
	for _, item := range opts.Items {
		sourceRoot, err := opts.Manifest.ResolveContentPath(opts.ManifestDir, item.Path)
		if err != nil {
			continue
		}
		root := filepath.Join(sourceRoot, item.Path)
		info, err := os.Stat(root)
		if err != nil {
			continue
		}
		if !info.IsDir() {
			continue
		}
		err = filepath.Walk(root, func(path string, fi os.FileInfo, err error) error {
			if err != nil {
				return nil
			}
			if fi.IsDir() {
				return watcher.Add(path)
			}
			return nil
		})
		if err != nil {
			return err
		}
	}
	return nil
}

func mergeDefaultExclude(extra []string) []string {
	return append(radio.DefaultExclude, extra...)
}

func findManifestItem(relPath string, items []manifest.ContentItem) *manifest.ContentItem {
	for i := range items {
		if strings.HasPrefix(relPath, items[i].Path+string(filepath.Separator)) || relPath == items[i].Path {
			return &items[i]
		}
	}
	return nil
}
