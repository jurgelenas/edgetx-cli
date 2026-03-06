package packages

import (
	"fmt"
	"strings"
)

// CheckConflicts returns an error if any of newPaths overlap with paths owned
// by already-installed packages. skipSource is excluded from checks (used
// during update to skip the package being updated).
//
// Overlap is determined by segment-based prefix matching (split on "/") to
// avoid false positives like "SCRIPTS/TOOLS" vs "SCRIPTS/TOOLSET".
func CheckConflicts(state *State, newPaths []string, skipSource string) error {
	installed := make(map[string]string) // path -> source
	for _, pkg := range state.Packages {
		if pkg.Source == skipSource {
			continue
		}
		for _, p := range pkg.Paths {
			installed[p] = pkg.Source
		}
	}

	var conflicts []string
	for _, np := range newPaths {
		npSegs := splitPath(np)
		for ip, owner := range installed {
			ipSegs := splitPath(ip)
			if segmentPrefixMatch(npSegs, ipSegs) {
				conflicts = append(conflicts, fmt.Sprintf("%q conflicts with %q (owned by %s)", np, ip, owner))
			}
		}
	}

	if len(conflicts) > 0 {
		return fmt.Errorf("path conflicts:\n  %s", strings.Join(conflicts, "\n  "))
	}
	return nil
}

func splitPath(p string) []string {
	return strings.Split(strings.TrimSuffix(p, "/"), "/")
}

// segmentPrefixMatch returns true if a is a prefix of b, b is a prefix of a,
// or they are equal — all at segment boundaries.
func segmentPrefixMatch(a, b []string) bool {
	shorter := a
	if len(b) < len(a) {
		shorter = b
	}
	for i := range shorter {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
