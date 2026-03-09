package source

import "strings"

// Source represents a parsed package source string. Source strings use "::" to
// separate an optional subpath and "@" to separate an optional version
// (e.g. "owner/repo::edgetx.c480x272.yml@branch").
type Source struct {
	Base    string // "owner/repo", "host.com/org/repo", or "/abs/path"
	SubPath string // "" or "edgetx.c480x272.yml"
	Version string // "" or "v1.0" or "main"
	IsLocal bool   // true for local paths and "local::" prefix
}

// Parse parses a raw source/query string into a Source.
func Parse(raw string) Source {
	if raw == "" {
		return Source{}
	}

	// "local::" prefix means a stored local source.
	if strings.HasPrefix(raw, "local::") {
		remainder := raw[len("local::"):]
		base, subPath := splitFirst(remainder, "::")
		return Source{Base: base, SubPath: subPath, IsLocal: true}
	}

	// Paths starting with . / ~ are local - never split on @.
	if raw[0] == '.' || raw[0] == '/' || raw[0] == '~' {
		base, subPath := splitFirst(raw, "::")
		return Source{Base: base, SubPath: subPath, IsLocal: true}
	}

	// Remote: split last @ for version, then first :: for subpath.
	base, version := splitLast(raw, "@")
	base, subPath := splitFirst(base, "::")
	return Source{Base: base, SubPath: subPath, Version: version}
}

// Canonical returns the source identifier without version - the format used in
// packages.yml (e.g. "owner/repo::sub" or "local::/path::sub").
func (s Source) Canonical() string {
	var b strings.Builder
	if s.IsLocal {
		b.WriteString("local::")
	}
	b.WriteString(s.Base)
	if s.SubPath != "" {
		b.WriteString("::")
		b.WriteString(s.SubPath)
	}
	return b.String()
}

// Full returns the canonical form plus "@version" if a version is set.
func (s Source) Full() string {
	c := s.Canonical()
	if s.Version != "" {
		return c + "@" + s.Version
	}
	return c
}

// WithSubPath returns a copy with the subpath set. A non-empty argument
// overrides any existing SubPath; an empty argument preserves it.
func (s Source) WithSubPath(p string) Source {
	if p != "" {
		s.SubPath = p
	}
	return s
}

// WithVersion returns a copy with the version set.
func (s Source) WithVersion(v string) Source {
	s.Version = v
	return s
}

// splitFirst splits s on the first occurrence of sep. If sep is not found,
// returns (s, "").
func splitFirst(s, sep string) (string, string) {
	if before, after, ok := strings.Cut(s, sep); ok {
		return before, after
	}
	return s, ""
}

// splitLast splits s on the last occurrence of sep. If sep is not found or only
// appears at position 0, returns (s, "").
func splitLast(s, sep string) (string, string) {
	if i := strings.LastIndex(s, sep); i > 0 {
		return s[:i], s[i+len(sep):]
	}
	return s, ""
}
