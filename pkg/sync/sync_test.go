package sync

import (
	"context"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jurgelenas/edgetx-cli/pkg/manifest"
	"github.com/stretchr/testify/assert"
)

func createSourceTree(t *testing.T, base string) {
	t.Helper()
	files := map[string]string{
		"SCRIPTS/ELRS/crsf.lua":             "-- crsf lib",
		"SCRIPTS/ELRS/shim.lua":             "-- shim lib",
		"SCRIPTS/TOOLS/ExpressLRS/elrs.lua": "-- elrs tool",
		"WIDGETS/ELRSTelemetry/main.lua":    "-- telemetry widget",
	}
	for relPath, content := range files {
		fullPath := filepath.Join(base, relPath)
		if !assert.NoError(t, os.MkdirAll(filepath.Dir(fullPath), 0o755)) {
			return
		}
		if !assert.NoError(t, os.WriteFile(fullPath, []byte(content), 0o644)) {
			return
		}
	}
}

func testItems() []manifest.ContentItem {
	return []manifest.ContentItem{
		{Name: "ELRS", Path: "SCRIPTS/ELRS"},
		{Name: "ExpressLRS", Path: "SCRIPTS/TOOLS/ExpressLRS"},
		{Name: "ELRSTelemetry", Path: "WIDGETS/ELRSTelemetry"},
	}
}

func testManifest() *manifest.Manifest {
	return &manifest.Manifest{
		Package: manifest.Package{Name: "test"},
		Libraries: []manifest.ContentItem{
			{Name: "ELRS", Path: "SCRIPTS/ELRS"},
		},
		Tools: []manifest.ContentItem{
			{Name: "ExpressLRS", Path: "SCRIPTS/TOOLS/ExpressLRS"},
		},
		Widgets: []manifest.ContentItem{
			{Name: "ELRSTelemetry", Path: "WIDGETS/ELRSTelemetry"},
		},
	}
}

func TestInitialSync_CopiesAllFiles(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	var copiedEvents []Event
	opts := Options{
		Manifest:    testManifest(),
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       testItems(),
		Callbacks: Callbacks{
			OnFileCopied: func(e Event) { copiedEvents = append(copiedEvents, e) },
		},
	}

	copied, err := InitialSync(opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 4, copied)
	assert.Len(t, copiedEvents, 4)

	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/shim.lua"))
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/TOOLS/ExpressLRS/elrs.lua"))
	assert.FileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/main.lua"))
}

func TestInitialSync_RespectsExclude(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	// Add a file that should be excluded.
	presetsPath := filepath.Join(srcDir, "WIDGETS/ELRSTelemetry/presets.txt")
	assert.NoError(t, os.WriteFile(presetsPath, []byte("user prefs"), 0o644))

	items := []manifest.ContentItem{
		{Name: "ELRSTelemetry", Path: "WIDGETS/ELRSTelemetry", Exclude: []string{"presets.txt"}},
	}

	m := &manifest.Manifest{
		Package: manifest.Package{Name: "test"},
		Widgets: items,
	}

	opts := Options{
		Manifest:    m,
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       items,
	}

	copied, err := InitialSync(opts)
	if !assert.NoError(t, err) {
		return
	}
	assert.Equal(t, 1, copied)
	assert.FileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/main.lua"))
	assert.NoFileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/presets.txt"))
}

func TestWatch_DetectsNewFile(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	var events []Event
	ready := make(chan struct{})
	done := make(chan error, 1)

	opts := Options{
		Manifest:    testManifest(),
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       testItems(),
		Callbacks: Callbacks{
			OnWatchReady: func() { close(ready) },
			OnSyncEvent: func(e Event) {
				events = append(events, e)
				cancel()
			},
		},
	}

	go func() { done <- Watch(ctx, opts) }()

	<-ready
	// Write a new file into a watched directory.
	newFile := filepath.Join(srcDir, "SCRIPTS/ELRS/new.lua")
	assert.NoError(t, os.WriteFile(newFile, []byte("-- new"), 0o644))

	assert.NoError(t, <-done)
	assert.NotEmpty(t, events)
	assert.Equal(t, "copy", events[0].Op)
	assert.Equal(t, filepath.Join("SCRIPTS", "ELRS", "new.lua"), events[0].RelPath)
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/new.lua"))
}

func TestWatch_DetectsModifiedFile(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	// Do initial sync so dest has original content.
	opts := Options{
		Manifest:    testManifest(),
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       testItems(),
	}
	_, err := InitialSync(opts)
	assert.NoError(t, err)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	var events []Event
	ready := make(chan struct{})
	done := make(chan error, 1)

	opts.Callbacks = Callbacks{
		OnWatchReady: func() { close(ready) },
		OnSyncEvent: func(e Event) {
			events = append(events, e)
			cancel()
		},
	}

	go func() { done <- Watch(ctx, opts) }()

	<-ready
	// Modify an existing file.
	assert.NoError(t, os.WriteFile(filepath.Join(srcDir, "SCRIPTS/ELRS/crsf.lua"), []byte("-- updated"), 0o644))

	assert.NoError(t, <-done)
	assert.NotEmpty(t, events)

	content, err := os.ReadFile(filepath.Join(destDir, "SCRIPTS/ELRS/crsf.lua"))
	assert.NoError(t, err)
	assert.Equal(t, "-- updated", string(content))
}

func TestWatch_IgnoresExcludedFile(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	items := []manifest.ContentItem{
		{Name: "ELRSTelemetry", Path: "WIDGETS/ELRSTelemetry", Exclude: []string{"*.txt"}},
	}

	m := &manifest.Manifest{
		Package: manifest.Package{Name: "test"},
		Widgets: items,
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	var eventCount atomic.Int32
	ready := make(chan struct{})
	done := make(chan error, 1)

	opts := Options{
		Manifest:    m,
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       items,
		Callbacks: Callbacks{
			OnWatchReady: func() { close(ready) },
			OnSyncEvent:  func(e Event) { eventCount.Add(1) },
		},
	}

	go func() { done <- Watch(ctx, opts) }()

	<-ready
	// Write a file that matches the exclude pattern.
	excludedFile := filepath.Join(srcDir, "WIDGETS/ELRSTelemetry/notes.txt")
	assert.NoError(t, os.WriteFile(excludedFile, []byte("excluded"), 0o644))

	// Wait enough time for the event to be processed if it were not excluded.
	time.Sleep(300 * time.Millisecond)
	cancel()

	assert.NoError(t, <-done)
	assert.Equal(t, int32(0), eventCount.Load(), "excluded file should not trigger sync event")
	assert.NoFileExists(t, filepath.Join(destDir, "WIDGETS/ELRSTelemetry/notes.txt"))
}

func TestWatch_CancelStops(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	ctx, cancel := context.WithCancel(context.Background())
	ready := make(chan struct{})
	done := make(chan error, 1)

	opts := Options{
		Manifest:    testManifest(),
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       testItems(),
		Callbacks: Callbacks{
			OnWatchReady: func() { close(ready) },
		},
	}

	go func() { done <- Watch(ctx, opts) }()

	<-ready
	cancel()

	select {
	case err := <-done:
		assert.NoError(t, err)
	case <-time.After(2 * time.Second):
		t.Fatal("Watch did not stop after context cancellation")
	}
}

func TestWatch_NewSubdirectory(t *testing.T) {
	srcDir := t.TempDir()
	destDir := t.TempDir()
	createSourceTree(t, srcDir)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	var events []Event
	ready := make(chan struct{})
	done := make(chan error, 1)

	opts := Options{
		Manifest:    testManifest(),
		ManifestDir: srcDir,
		TargetDir:   destDir,
		Items:       testItems(),
		Callbacks: Callbacks{
			OnWatchReady: func() { close(ready) },
			OnSyncEvent: func(e Event) {
				if e.Op == "copy" {
					events = append(events, e)
					cancel()
				}
			},
		},
	}

	go func() { done <- Watch(ctx, opts) }()

	<-ready
	// Create a new subdirectory with a file.
	newDir := filepath.Join(srcDir, "SCRIPTS/ELRS/utils")
	assert.NoError(t, os.MkdirAll(newDir, 0o755))
	// Small delay to let the watcher pick up the new directory.
	time.Sleep(100 * time.Millisecond)
	assert.NoError(t, os.WriteFile(filepath.Join(newDir, "helper.lua"), []byte("-- helper"), 0o644))

	assert.NoError(t, <-done)
	assert.NotEmpty(t, events)
	assert.FileExists(t, filepath.Join(destDir, "SCRIPTS/ELRS/utils/helper.lua"))
}
