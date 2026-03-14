package simulator

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/jurgelenas/edgetx-cli/pkg/logging"
)

const (
	catalogURL  = "https://edgetx-simulator.pages.dev/radios.json"
	wasmBaseURL = "https://edgetx-simulator.pages.dev/"
	catalogTTL  = 1 * time.Hour
)

// RadioDef describes a radio model from the simulator catalog.
type RadioDef struct {
	Name     string      `json:"name"`
	WASM     string      `json:"wasm"`
	Display  DisplayDef  `json:"display"`
	Inputs   []InputDef  `json:"inputs"`
	Switches []SwitchDef `json:"switches"`
	Trims    []TrimDef   `json:"trims"`
	Keys     []KeyDef    `json:"keys"`
}

// DisplayDef describes the LCD dimensions and color depth.
type DisplayDef struct {
	W     int `json:"w"`
	H     int `json:"h"`
	Depth int `json:"depth"`
}

// InputDef describes an analog input (stick, pot, slider).
type InputDef struct {
	Name    string `json:"name"`
	Type    string `json:"type"`
	Label   string `json:"label"`
	Default string `json:"default"`
}

// SwitchDef describes a physical switch.
type SwitchDef struct {
	Name    string `json:"name"`
	Type    string `json:"type"`
	Default string `json:"default"`
}

// TrimDef describes a trim button pair.
type TrimDef struct {
	Name string `json:"name"`
}

// KeyDef describes a button/key on the radio.
type KeyDef struct {
	Key   string `json:"key"`
	Label string `json:"label"`
	Side  string `json:"side"`
}

// Key returns a URL-safe slug derived from the radio name.
func (r *RadioDef) Key() string {
	s := strings.ToLower(r.Name)
	s = strings.ReplaceAll(s, " ", "-")
	s = strings.ReplaceAll(s, "(", "")
	s = strings.ReplaceAll(s, ")", "")
	return s
}

func cacheDir() (string, error) {
	dir, err := os.UserCacheDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "edgetx-cli", "simulator"), nil
}

// FetchCatalog downloads and caches the radios.json catalog.
func FetchCatalog() ([]RadioDef, error) {
	cache, err := cacheDir()
	if err != nil {
		return nil, fmt.Errorf("cache directory: %w", err)
	}

	catalogPath := filepath.Join(cache, "radios.json")

	// Check cache freshness.
	if info, err := os.Stat(catalogPath); err == nil {
		if time.Since(info.ModTime()) < catalogTTL {
			logging.Debugf("using cached catalog %s", catalogPath)
			return loadCatalog(catalogPath)
		}
	}

	logging.Debugf("fetching catalog from %s", catalogURL)

	resp, err := http.Get(catalogURL)
	if err != nil {
		// Fall back to cache if network fails.
		if radios, cacheErr := loadCatalog(catalogPath); cacheErr == nil {
			logging.Warn("using stale cache after network error")
			return radios, nil
		}
		return nil, fmt.Errorf("fetching radio catalog: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("fetching radio catalog: HTTP %d", resp.StatusCode)
	}

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("reading catalog response: %w", err)
	}

	if err := os.MkdirAll(cache, 0o755); err != nil {
		return nil, fmt.Errorf("creating cache directory: %w", err)
	}
	if err := os.WriteFile(catalogPath, data, 0o644); err != nil {
		logging.WithError(err).Warn("failed to cache catalog")
	}

	var radios []RadioDef
	if err := json.Unmarshal(data, &radios); err != nil {
		return nil, fmt.Errorf("parsing radio catalog: %w", err)
	}
	return radios, nil
}

func loadCatalog(path string) ([]RadioDef, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var radios []RadioDef
	if err := json.Unmarshal(data, &radios); err != nil {
		return nil, fmt.Errorf("parsing cached catalog: %w", err)
	}
	return radios, nil
}

// FindRadio finds a radio by name or WASM filename slug (case-insensitive).
func FindRadio(catalog []RadioDef, query string) (*RadioDef, error) {
	q := strings.ToLower(query)

	// Exact name match first.
	for i := range catalog {
		if strings.ToLower(catalog[i].Name) == q {
			return &catalog[i], nil
		}
	}

	// Match by key (slug).
	for i := range catalog {
		if catalog[i].Key() == q {
			return &catalog[i], nil
		}
	}

	// Match by WASM filename (without extension).
	for i := range catalog {
		slug := strings.TrimSuffix(catalog[i].WASM, ".wasm")
		if strings.ToLower(slug) == q {
			return &catalog[i], nil
		}
	}

	// Substring match.
	var matches []*RadioDef
	for i := range catalog {
		if strings.Contains(strings.ToLower(catalog[i].Name), q) {
			matches = append(matches, &catalog[i])
		}
	}

	switch len(matches) {
	case 0:
		return nil, fmt.Errorf("no radio found matching %q", query)
	case 1:
		return matches[0], nil
	default:
		names := make([]string, len(matches))
		for i, m := range matches {
			names[i] = m.Name
		}
		return nil, fmt.Errorf("ambiguous query %q matches: %s", query, strings.Join(names, ", "))
	}
}

// EnsureWASM downloads the WASM binary for a radio if not already cached.
// Returns the local file path. The onProgress callback receives bytes written so far.
func EnsureWASM(radio *RadioDef, onProgress func(downloaded, total int64)) (string, error) {
	cache, err := cacheDir()
	if err != nil {
		return "", err
	}

	wasmDir := filepath.Join(cache, "wasm")
	wasmPath := filepath.Join(wasmDir, radio.WASM)

	// Check cache — validate it's actually a WASM file (magic: \x00asm).
	if info, err := os.Stat(wasmPath); err == nil && info.Size() > 4 {
		if isValidWASM(wasmPath) {
			logging.Debugf("WASM cached at %s", wasmPath)
			return wasmPath, nil
		}
		logging.Debugf("cached file is not valid WASM, re-downloading")
		os.Remove(wasmPath)
	}

	url := wasmBaseURL + radio.WASM

	// HEAD check — verify file exists. Note: Cloudflare Pages may serve
	// WASM files with text/html content-type, so we can't rely on
	// content-type alone. We check availability and validate magic bytes
	// after download.
	headResp, err := http.Head(url)
	if err != nil {
		return "", fmt.Errorf("checking WASM availability: %w", err)
	}
	headResp.Body.Close()

	if headResp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("WASM file %s is not available (HTTP %d)", radio.WASM, headResp.StatusCode)
	}

	logging.Debugf("downloading %s", url)

	resp, err := http.Get(url)
	if err != nil {
		return "", fmt.Errorf("downloading WASM: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("downloading WASM: HTTP %d", resp.StatusCode)
	}

	if err := os.MkdirAll(wasmDir, 0o755); err != nil {
		return "", fmt.Errorf("creating WASM cache dir: %w", err)
	}

	tmpFile, err := os.CreateTemp(wasmDir, "*.wasm.tmp")
	if err != nil {
		return "", fmt.Errorf("creating temp file: %w", err)
	}
	tmpPath := tmpFile.Name()
	defer func() {
		tmpFile.Close()
		os.Remove(tmpPath)
	}()

	var reader io.Reader = resp.Body
	if onProgress != nil {
		reader = &progressReader{r: resp.Body, total: resp.ContentLength, onProgress: onProgress}
	}

	if _, err := io.Copy(tmpFile, reader); err != nil {
		return "", fmt.Errorf("writing WASM file: %w", err)
	}
	tmpFile.Close()

	if err := os.Rename(tmpPath, wasmPath); err != nil {
		return "", fmt.Errorf("moving WASM file to cache: %w", err)
	}

	// Validate it's actually a WASM file.
	if !isValidWASM(wasmPath) {
		os.Remove(wasmPath)
		return "", fmt.Errorf("downloaded file for %s is not a valid WASM binary — "+
			"this radio may not be available yet", radio.Name)
	}

	return wasmPath, nil
}

type progressReader struct {
	r          io.Reader
	total      int64
	downloaded int64
	onProgress func(downloaded, total int64)
}

func (pr *progressReader) Read(p []byte) (int, error) {
	n, err := pr.r.Read(p)
	pr.downloaded += int64(n)
	pr.onProgress(pr.downloaded, pr.total)
	return n, err
}

// isValidWASM checks if a file starts with the WASM magic bytes (\x00asm).
func isValidWASM(path string) bool {
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()
	magic := make([]byte, 4)
	if _, err := io.ReadFull(f, magic); err != nil {
		return false
	}
	return magic[0] == 0x00 && magic[1] == 'a' && magic[2] == 's' && magic[3] == 'm'
}
