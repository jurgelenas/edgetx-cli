package cmd

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestInsertSubPath(t *testing.T) {
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
			name:    "local path unchanged",
			query:   "./my-package",
			subPath: "edgetx.yml",
			want:    "./my-package::edgetx.yml",
		},
		{
			name:    "absolute path unchanged",
			query:   "/home/user/pkg",
			subPath: "edgetx.yml",
			want:    "/home/user/pkg::edgetx.yml",
		},
		{
			name:    "home path unchanged",
			query:   "~/pkg",
			subPath: "edgetx.yml",
			want:    "~/pkg::edgetx.yml",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := insertSubPath(tt.query, tt.subPath)
			assert.Equal(t, tt.want, got)
		})
	}
}
