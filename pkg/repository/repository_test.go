package repository

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestParsePackageRef_GitHubShorthand(t *testing.T) {
	ref, err := ParsePackageRef("ExpressLRS/Lua-Scripts")
	assert.NoError(t, err)
	assert.Equal(t, "", ref.Host)
	assert.Equal(t, "ExpressLRS", ref.Owner)
	assert.Equal(t, "Lua-Scripts", ref.Repo)
	assert.Equal(t, "", ref.Version)
	assert.False(t, ref.IsLocal)
}

func TestParsePackageRef_GitHubWithTag(t *testing.T) {
	ref, err := ParsePackageRef("ExpressLRS/Lua-Scripts@v1.6.0")
	assert.NoError(t, err)
	assert.Equal(t, "ExpressLRS", ref.Owner)
	assert.Equal(t, "Lua-Scripts", ref.Repo)
	assert.Equal(t, "v1.6.0", ref.Version)
}

func TestParsePackageRef_GitHubWithBranch(t *testing.T) {
	ref, err := ParsePackageRef("Org/Repo@main")
	assert.NoError(t, err)
	assert.Equal(t, "main", ref.Version)
}

func TestParsePackageRef_GitHubWithCommit(t *testing.T) {
	ref, err := ParsePackageRef("Org/Repo@abc123def")
	assert.NoError(t, err)
	assert.Equal(t, "abc123def", ref.Version)
}

func TestParsePackageRef_FullURLNoScheme(t *testing.T) {
	ref, err := ParsePackageRef("gitea.example.com/user/repo")
	assert.NoError(t, err)
	assert.Equal(t, "gitea.example.com", ref.Host)
	assert.Equal(t, "user", ref.Owner)
	assert.Equal(t, "repo", ref.Repo)
}

func TestParsePackageRef_FullURLWithScheme(t *testing.T) {
	ref, err := ParsePackageRef("https://gitlab.com/org/repo@v1.0")
	assert.NoError(t, err)
	assert.Equal(t, "gitlab.com", ref.Host)
	assert.Equal(t, "org", ref.Owner)
	assert.Equal(t, "repo", ref.Repo)
	assert.Equal(t, "v1.0", ref.Version)
}

func TestParsePackageRef_FullGitHubURL(t *testing.T) {
	ref, err := ParsePackageRef("github.com/ExpressLRS/Lua-Scripts")
	assert.NoError(t, err)
	assert.Equal(t, "github.com", ref.Host)
	assert.Equal(t, "ExpressLRS", ref.Owner)
	assert.Equal(t, "Lua-Scripts", ref.Repo)
}

func TestParsePackageRef_LocalRelativePath(t *testing.T) {
	ref, err := ParsePackageRef("./my-project")
	assert.NoError(t, err)
	assert.True(t, ref.IsLocal)
	assert.True(t, filepath.IsAbs(ref.LocalPath))
}

func TestParsePackageRef_LocalAbsolutePath(t *testing.T) {
	ref, err := ParsePackageRef("/home/user/project")
	assert.NoError(t, err)
	assert.True(t, ref.IsLocal)
	assert.Equal(t, "/home/user/project", ref.LocalPath)
}

func TestParsePackageRef_LocalDot(t *testing.T) {
	ref, err := ParsePackageRef(".")
	assert.NoError(t, err)
	assert.True(t, ref.IsLocal)
	cwd, _ := os.Getwd()
	assert.Equal(t, cwd, ref.LocalPath)
}

func TestParsePackageRef_Empty(t *testing.T) {
	_, err := ParsePackageRef("")
	assert.Error(t, err)
}

func TestParsePackageRef_SingleSegment(t *testing.T) {
	_, err := ParsePackageRef("just-a-name")
	assert.Error(t, err)
}

func TestParsePackageRef_TooManyAt(t *testing.T) {
	_, err := ParsePackageRef("Org/Repo@v1@v2")
	assert.Error(t, err)
}

func TestPackageRef_Canonical_GitHub(t *testing.T) {
	ref := PackageRef{Owner: "ExpressLRS", Repo: "Lua-Scripts"}
	assert.Equal(t, "ExpressLRS/Lua-Scripts", ref.Canonical())
}

func TestPackageRef_Canonical_FullURL(t *testing.T) {
	ref := PackageRef{Host: "gitea.example.com", Owner: "user", Repo: "repo"}
	assert.Equal(t, "gitea.example.com/user/repo", ref.Canonical())
}

func TestPackageRef_Canonical_Local(t *testing.T) {
	ref := PackageRef{IsLocal: true, LocalPath: "/abs/path"}
	assert.Equal(t, "local::/abs/path", ref.Canonical())
}

func TestPackageRef_CloneURL_GitHub(t *testing.T) {
	ref := PackageRef{Owner: "ExpressLRS", Repo: "Lua-Scripts"}
	assert.Equal(t, "https://github.com/ExpressLRS/Lua-Scripts.git", ref.CloneURL())
}

func TestPackageRef_CloneURL_CustomHost(t *testing.T) {
	ref := PackageRef{Host: "gitea.example.com", Owner: "user", Repo: "repo"}
	assert.Equal(t, "https://gitea.example.com/user/repo.git", ref.CloneURL())
}

func TestParsePackageRef_TrailingGitSuffix(t *testing.T) {
	ref, err := ParsePackageRef("https://github.com/Org/Repo.git")
	assert.NoError(t, err)
	assert.Equal(t, "Org", ref.Owner)
	assert.Equal(t, "Repo", ref.Repo)
}
