package repository

import (
	"os"
	"testing"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/stretchr/testify/assert"
)

func TestSortSemverTags(t *testing.T) {
	tags := []string{"v1.0.0", "v2.0.0", "v1.5.0", "v0.1.0"}
	sorted := SortSemverTags(tags)
	assert.Equal(t, []string{"v2.0.0", "v1.5.0", "v1.0.0", "v0.1.0"}, sorted)
}

func TestSortSemverTags_WithPrefix(t *testing.T) {
	tags := []string{"v1.0.0", "1.5.0"}
	sorted := SortSemverTags(tags)
	assert.Equal(t, []string{"1.5.0", "v1.0.0"}, sorted)
}

func TestSortSemverTags_PreRelease(t *testing.T) {
	tags := []string{"v1.0.0-beta", "v1.0.0"}
	sorted := SortSemverTags(tags)
	assert.Equal(t, "v1.0.0", sorted[0])
	assert.Equal(t, "v1.0.0-beta", sorted[1])
}

func TestSortSemverTags_NonSemverIgnored(t *testing.T) {
	tags := []string{"v1.0.0", "release-2024", "v2.0.0", "latest"}
	sorted := SortSemverTags(tags)
	assert.Equal(t, []string{"v2.0.0", "v1.0.0"}, sorted)
}

// createTestRepo creates a bare-like local repo for testing version resolution.
func createTestRepo(t *testing.T) (*git.Repository, string) {
	t.Helper()
	dir := t.TempDir()

	repo, err := git.PlainInit(dir, false)
	if !assert.NoError(t, err) {
		t.FailNow()
	}

	// Create an initial commit.
	wt, err := repo.Worktree()
	assert.NoError(t, err)

	f, err := os.Create(dir + "/README.md")
	assert.NoError(t, err)
	f.WriteString("# Test")
	f.Close()

	wt.Add("README.md")
	_, err = wt.Commit("initial commit", &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test",
			Email: "test@test.com",
			When:  time.Now(),
		},
	})
	assert.NoError(t, err)

	return repo, dir
}

func TestResolveVersion_LatestTag(t *testing.T) {
	repo, _ := createTestRepo(t)

	head, _ := repo.Head()

	// Create tags.
	repo.CreateTag("v1.0.0", head.Hash(), nil)
	repo.CreateTag("v2.0.0", head.Hash(), nil)

	rv, err := ResolveVersion(repo, "")
	assert.NoError(t, err)
	assert.Equal(t, "tag", rv.Channel)
	assert.Equal(t, "v2.0.0", rv.Version)
}

func TestResolveVersion_ExplicitTag(t *testing.T) {
	repo, _ := createTestRepo(t)
	head, _ := repo.Head()

	repo.CreateTag("v1.0.0", head.Hash(), nil)

	rv, err := ResolveVersion(repo, "v1.0.0")
	assert.NoError(t, err)
	assert.Equal(t, "tag", rv.Channel)
	assert.Equal(t, "v1.0.0", rv.Version)
}

func TestResolveVersion_Branch(t *testing.T) {
	repo, _ := createTestRepo(t)
	head, _ := repo.Head()

	// The default branch (master or main) should be resolvable.
	branchName := head.Name().Short()

	rv, err := ResolveVersion(repo, branchName)
	assert.NoError(t, err)
	assert.Equal(t, "branch", rv.Channel)
	assert.Equal(t, branchName, rv.Version)
}

func TestResolveVersion_NoTags(t *testing.T) {
	repo, _ := createTestRepo(t)

	rv, err := ResolveVersion(repo, "")
	assert.NoError(t, err)
	assert.Equal(t, "branch", rv.Channel)
	assert.NotEmpty(t, rv.Hash)
}

func TestResolveVersion_FullCommitSHA(t *testing.T) {
	repo, _ := createTestRepo(t)
	head, _ := repo.Head()

	rv, err := ResolveVersion(repo, head.Hash().String())
	assert.NoError(t, err)
	assert.Equal(t, "commit", rv.Channel)
	assert.Equal(t, head.Hash(), rv.Hash)
}

func TestResolveVersion_ShortCommitSHA(t *testing.T) {
	repo, _ := createTestRepo(t)
	head, _ := repo.Head()

	short := head.Hash().String()[:7]
	rv, err := ResolveVersion(repo, short)
	assert.NoError(t, err)
	assert.Equal(t, "commit", rv.Channel)
}

func TestResolveVersion_AmbiguousRef(t *testing.T) {
	repo, _ := createTestRepo(t)
	head, _ := repo.Head()

	repo.CreateTag("v1", head.Hash(), nil)

	// "v1" should match tag first.
	rv, err := ResolveVersion(repo, "v1")
	assert.NoError(t, err)
	assert.Equal(t, "tag", rv.Channel)
}

func TestResolveVersion_InvalidRef(t *testing.T) {
	repo, _ := createTestRepo(t)

	_, err := ResolveVersion(repo, "nonexistent")
	assert.Error(t, err)
}

func TestDetectRefType(t *testing.T) {
	repo, _ := createTestRepo(t)
	head, _ := repo.Head()

	repo.CreateTag("v1.0.0", head.Hash(), nil)

	assert.Equal(t, "tag", DetectRefType(repo, "v1.0.0"))
	assert.Equal(t, "branch", DetectRefType(repo, head.Name().Short()))
	assert.Equal(t, "commit", DetectRefType(repo, head.Hash().String()[:7]))
}

// Suppress unused import warning for plumbing in tests.
var _ = plumbing.HEAD
