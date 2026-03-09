package source

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestParse(t *testing.T) {
	tests := []struct {
		name string
		raw  string
		want Source
	}{
		{
			name: "empty",
			raw:  "",
			want: Source{},
		},
		{
			name: "simple remote",
			raw:  "owner/repo",
			want: Source{Base: "owner/repo"},
		},
		{
			name: "remote with version",
			raw:  "owner/repo@v1.0",
			want: Source{Base: "owner/repo", Version: "v1.0"},
		},
		{
			name: "remote with subpath",
			raw:  "owner/repo::edgetx.c480x272.yml",
			want: Source{Base: "owner/repo", SubPath: "edgetx.c480x272.yml"},
		},
		{
			name: "remote with subpath and version",
			raw:  "owner/repo::edgetx.c480x272.yml@v1.0",
			want: Source{Base: "owner/repo", SubPath: "edgetx.c480x272.yml", Version: "v1.0"},
		},
		{
			name: "remote with branch version",
			raw:  "ExpressLRS/Lua-Scripts@main",
			want: Source{Base: "ExpressLRS/Lua-Scripts", Version: "main"},
		},
		{
			name: "full host remote",
			raw:  "gitea.example.com/user/repo@v1.0",
			want: Source{Base: "gitea.example.com/user/repo", Version: "v1.0"},
		},
		{
			name: "plain name no slash",
			raw:  "expresslrs",
			want: Source{Base: "expresslrs"},
		},
		{
			name: "local:: prefix",
			raw:  "local::/home/user/project",
			want: Source{Base: "/home/user/project", IsLocal: true},
		},
		{
			name: "local:: prefix with subpath",
			raw:  "local::/home/user/project::edgetx.c480x272.yml",
			want: Source{Base: "/home/user/project", SubPath: "edgetx.c480x272.yml", IsLocal: true},
		},
		{
			name: "dot path",
			raw:  ".",
			want: Source{Base: ".", IsLocal: true},
		},
		{
			name: "relative path",
			raw:  "./my-project",
			want: Source{Base: "./my-project", IsLocal: true},
		},
		{
			name: "relative path with subpath",
			raw:  "./my-project::edgetx.yml",
			want: Source{Base: "./my-project", SubPath: "edgetx.yml", IsLocal: true},
		},
		{
			name: "absolute path",
			raw:  "/home/user/pkg",
			want: Source{Base: "/home/user/pkg", IsLocal: true},
		},
		{
			name: "absolute path with subpath",
			raw:  "/home/user/pkg::edgetx.yml",
			want: Source{Base: "/home/user/pkg", SubPath: "edgetx.yml", IsLocal: true},
		},
		{
			name: "home path",
			raw:  "~/pkg",
			want: Source{Base: "~/pkg", IsLocal: true},
		},
		{
			name: "local path with @ in directory name",
			raw:  "./my@project",
			want: Source{Base: "./my@project", IsLocal: true},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := Parse(tt.raw)
			assert.Equal(t, tt.want, got)
		})
	}
}

func TestCanonical(t *testing.T) {
	tests := []struct {
		name string
		src  Source
		want string
	}{
		{
			name: "simple remote",
			src:  Source{Base: "owner/repo"},
			want: "owner/repo",
		},
		{
			name: "remote with subpath",
			src:  Source{Base: "owner/repo", SubPath: "edgetx.c480x272.yml"},
			want: "owner/repo::edgetx.c480x272.yml",
		},
		{
			name: "remote with version ignored",
			src:  Source{Base: "owner/repo", Version: "v1.0"},
			want: "owner/repo",
		},
		{
			name: "local",
			src:  Source{Base: "/home/user/project", IsLocal: true},
			want: "local::/home/user/project",
		},
		{
			name: "local with subpath",
			src:  Source{Base: "/home/user/project", SubPath: "edgetx.yml", IsLocal: true},
			want: "local::/home/user/project::edgetx.yml",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			assert.Equal(t, tt.want, tt.src.Canonical())
		})
	}
}

func TestFull(t *testing.T) {
	tests := []struct {
		name string
		src  Source
		want string
	}{
		{
			name: "no version",
			src:  Source{Base: "owner/repo"},
			want: "owner/repo",
		},
		{
			name: "with version",
			src:  Source{Base: "owner/repo", Version: "v1.0"},
			want: "owner/repo@v1.0",
		},
		{
			name: "with subpath and version",
			src:  Source{Base: "owner/repo", SubPath: "sub", Version: "v1.0"},
			want: "owner/repo::sub@v1.0",
		},
		{
			name: "local ignores version field",
			src:  Source{Base: "/path", IsLocal: true},
			want: "local::/path",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			assert.Equal(t, tt.want, tt.src.Full())
		})
	}
}

func TestWithSubPath(t *testing.T) {
	tests := []struct {
		name    string
		src     Source
		subPath string
		want    string
	}{
		{
			name:    "empty subpath preserves existing",
			src:     Source{Base: "owner/repo", SubPath: "existing"},
			subPath: "",
			want:    "existing",
		},
		{
			name:    "non-empty subpath overrides",
			src:     Source{Base: "owner/repo", SubPath: "existing"},
			subPath: "override",
			want:    "override",
		},
		{
			name:    "sets subpath when none exists",
			src:     Source{Base: "owner/repo"},
			subPath: "edgetx.yml",
			want:    "edgetx.yml",
		},
		{
			name:    "empty on empty is no-op",
			src:     Source{Base: "owner/repo"},
			subPath: "",
			want:    "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.src.WithSubPath(tt.subPath)
			assert.Equal(t, tt.want, got.SubPath)
		})
	}
}

func TestWithVersion(t *testing.T) {
	src := Source{Base: "owner/repo"}
	got := src.WithVersion("v2.0")
	assert.Equal(t, "v2.0", got.Version)
	assert.Equal(t, "", src.Version, "original should be unchanged")
}

func TestRoundTrip(t *testing.T) {
	inputs := []string{
		"owner/repo",
		"owner/repo@v1.0",
		"owner/repo::sub",
		"owner/repo::sub@v1.0",
		"expresslrs",
	}

	for _, input := range inputs {
		t.Run(input, func(t *testing.T) {
			s := Parse(input)
			assert.Equal(t, s.Full(), Parse(s.Full()).Full())
		})
	}
}

// TestWithSubPath_InsertSubPath covers the old insertSubPath test cases.
func TestWithSubPath_InsertSubPath(t *testing.T) {
	tests := []struct {
		name    string
		query   string
		subPath string
		want    string
	}{
		{
			name:    "empty subpath is a no-op",
			query:   "owner/repo@v1.0",
			subPath: "",
			want:    "owner/repo@v1.0",
		},
		{
			name:    "no version",
			query:   "owner/repo",
			subPath: "edgetx.c480x272.yml",
			want:    "owner/repo::edgetx.c480x272.yml",
		},
		{
			name:    "with version",
			query:   "owner/repo@edgetx-package",
			subPath: "edgetx.c480x272.yml",
			want:    "owner/repo::edgetx.c480x272.yml@edgetx-package",
		},
		{
			name:    "with semver version",
			query:   "owner/repo@v1.2.3",
			subPath: "sub/dir",
			want:    "owner/repo::sub/dir@v1.2.3",
		},
		{
			name:    "local path",
			query:   "./my-package",
			subPath: "edgetx.yml",
			want:    "local::./my-package::edgetx.yml",
		},
		{
			name:    "absolute path",
			query:   "/home/user/pkg",
			subPath: "edgetx.yml",
			want:    "local::/home/user/pkg::edgetx.yml",
		},
		{
			name:    "home path",
			query:   "~/pkg",
			subPath: "edgetx.yml",
			want:    "local::~/pkg::edgetx.yml",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := Parse(tt.query).WithSubPath(tt.subPath).Full()
			assert.Equal(t, tt.want, got)
		})
	}
}
