package repository

import (
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
)

// PackageRef represents a parsed package reference.
type PackageRef struct {
	Host      string // "" for GitHub, or "gitea.example.com"
	Owner     string // "ExpressLRS"
	Repo      string // "Lua-Scripts"
	Version   string // "v1.6.0", "main", "abc123", or "" (latest)
	IsLocal   bool   // true if source is a local directory path
	LocalPath string // absolute path when IsLocal=true
}

// ParsePackageRef parses a raw package reference string into a PackageRef.
//
// Accepted formats:
//   - Local paths: ".", "./dir", "/abs/path", "~/dir"
//   - GitHub shorthand: "Org/Repo", "Org/Repo@v1.0.0"
//   - Full URL: "host.com/org/repo", "https://host.com/org/repo@v1.0.0"
func ParsePackageRef(raw string) (PackageRef, error) {
	if raw == "" {
		return PackageRef{}, fmt.Errorf("empty package reference")
	}

	// Local path detection: starts with ".", "/", or "~"
	if strings.HasPrefix(raw, ".") || strings.HasPrefix(raw, "/") || strings.HasPrefix(raw, "~") {
		return parseLocal(raw)
	}

	// Check if it's an existing local directory (e.g., a relative path without "./")
	if info, err := os.Stat(raw); err == nil && info.IsDir() {
		return parseLocal(raw)
	}

	return parseRemote(raw)
}

func parseLocal(raw string) (PackageRef, error) {
	path := raw

	// Expand ~ to home directory
	if strings.HasPrefix(path, "~") {
		home, err := os.UserHomeDir()
		if err != nil {
			return PackageRef{}, fmt.Errorf("expanding home directory: %w", err)
		}
		path = filepath.Join(home, path[1:])
	}

	abs, err := filepath.Abs(path)
	if err != nil {
		return PackageRef{}, fmt.Errorf("resolving path %q: %w", raw, err)
	}

	return PackageRef{
		IsLocal:   true,
		LocalPath: abs,
	}, nil
}

func parseRemote(raw string) (PackageRef, error) {
	// Split on @ for version, but only the last @
	if strings.Count(raw, "@") > 1 {
		return PackageRef{}, fmt.Errorf("invalid package reference %q: multiple @ symbols", raw)
	}

	version := ""
	remainder := raw
	if idx := strings.LastIndex(raw, "@"); idx != -1 {
		version = raw[idx+1:]
		remainder = raw[:idx]
		if version == "" {
			return PackageRef{}, fmt.Errorf("invalid package reference %q: empty version after @", raw)
		}
	}

	// Strip scheme if present
	cleanPath := remainder
	if u, err := url.Parse(remainder); err == nil && u.Scheme != "" {
		cleanPath = u.Host + u.Path
	}

	// Remove trailing .git
	cleanPath = strings.TrimSuffix(cleanPath, ".git")
	// Remove trailing slash
	cleanPath = strings.TrimSuffix(cleanPath, "/")

	parts := strings.Split(cleanPath, "/")

	switch len(parts) {
	case 2:
		// GitHub shorthand: Org/Repo
		if parts[0] == "" || parts[1] == "" {
			return PackageRef{}, fmt.Errorf("invalid package reference %q: empty owner or repo", raw)
		}
		return PackageRef{
			Owner:   parts[0],
			Repo:    parts[1],
			Version: version,
		}, nil
	case 3:
		// Full URL: host.com/org/repo
		if parts[0] == "" || parts[1] == "" || parts[2] == "" {
			return PackageRef{}, fmt.Errorf("invalid package reference %q: empty host, owner, or repo", raw)
		}
		// Verify host looks like a hostname (contains a dot)
		if !strings.Contains(parts[0], ".") {
			return PackageRef{}, fmt.Errorf("invalid package reference %q: expected host.com/org/repo or Org/Repo format", raw)
		}
		return PackageRef{
			Host:    parts[0],
			Owner:   parts[1],
			Repo:    parts[2],
			Version: version,
		}, nil
	default:
		return PackageRef{}, fmt.Errorf("invalid package reference %q: expected Org/Repo or host.com/org/repo format", raw)
	}
}

// Canonical returns the canonical identifier for this package.
// For GitHub: "Org/Repo", for full URL: "host.com/org/repo", for local: "local::/abs/path".
func (r PackageRef) Canonical() string {
	if r.IsLocal {
		return "local::" + r.LocalPath
	}
	if r.Host != "" {
		return r.Host + "/" + r.Owner + "/" + r.Repo
	}
	return r.Owner + "/" + r.Repo
}

// CloneURL returns the HTTPS clone URL for this package.
func (r PackageRef) CloneURL() string {
	host := r.Host
	if host == "" {
		host = "github.com"
	}
	return "https://" + host + "/" + r.Owner + "/" + r.Repo + ".git"
}
