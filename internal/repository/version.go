package repository

import (
	"fmt"
	"sort"
	"strings"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"golang.org/x/mod/semver"
)

// ResolvedVersion holds the result of version resolution.
type ResolvedVersion struct {
	Channel string // "tag", "branch", or "commit"
	Version string // tag name or branch name (empty for commit)
	Hash    plumbing.Hash
}

// ResolveVersion resolves a version string to a concrete commit in the given
// repository. If version is empty, the latest semver tag is used; if no tags
// exist, the default branch HEAD is used.
func ResolveVersion(repo *git.Repository, version string) (ResolvedVersion, error) {
	if version == "" {
		return resolveLatest(repo)
	}

	// Try exact tag match first.
	if rv, err := resolveTag(repo, version); err == nil {
		return rv, nil
	}

	// Try branch.
	if rv, err := resolveBranch(repo, version); err == nil {
		return rv, nil
	}

	// Try commit SHA (full or prefix).
	if rv, err := resolveCommit(repo, version); err == nil {
		return rv, nil
	}

	return ResolvedVersion{}, fmt.Errorf("could not resolve version %q: not a tag, branch, or commit", version)
}

// SortSemverTags sorts tags in descending semver order. Non-semver tags are
// filtered out. Tags without 'v' prefix are normalized for comparison.
func SortSemverTags(tags []string) []string {
	var valid []string
	for _, tag := range tags {
		normalized := tag
		if !strings.HasPrefix(normalized, "v") {
			normalized = "v" + normalized
		}
		if semver.IsValid(normalized) {
			valid = append(valid, tag)
		}
	}

	sort.Slice(valid, func(i, j int) bool {
		a, b := valid[i], valid[j]
		if !strings.HasPrefix(a, "v") {
			a = "v" + a
		}
		if !strings.HasPrefix(b, "v") {
			b = "v" + b
		}
		return semver.Compare(a, b) > 0
	})

	return valid
}

// DetectRefType returns the channel type ("tag", "branch", or "commit") for a
// given ref string against the repository.
func DetectRefType(repo *git.Repository, ref string) string {
	if _, err := resolveTag(repo, ref); err == nil {
		return "tag"
	}
	if _, err := resolveBranch(repo, ref); err == nil {
		return "branch"
	}
	return "commit"
}

func resolveLatest(repo *git.Repository) (ResolvedVersion, error) {
	tags, err := listTagNames(repo)
	if err != nil {
		return ResolvedVersion{}, err
	}

	sorted := SortSemverTags(tags)
	if len(sorted) > 0 {
		return resolveTag(repo, sorted[0])
	}

	// Fall back to default branch HEAD.
	head, err := repo.Head()
	if err != nil {
		return ResolvedVersion{}, fmt.Errorf("getting HEAD: %w", err)
	}

	branchName := head.Name().Short()
	return ResolvedVersion{
		Channel: "branch",
		Version: branchName,
		Hash:    head.Hash(),
	}, nil
}

func resolveTag(repo *git.Repository, name string) (ResolvedVersion, error) {
	ref, err := repo.Tag(name)
	if err != nil {
		return ResolvedVersion{}, err
	}

	hash, err := repo.ResolveRevision(plumbing.Revision(ref.Name()))
	if err != nil {
		return ResolvedVersion{}, err
	}

	return ResolvedVersion{
		Channel: "tag",
		Version: name,
		Hash:    *hash,
	}, nil
}

func resolveBranch(repo *git.Repository, name string) (ResolvedVersion, error) {
	ref, err := repo.Reference(plumbing.NewRemoteReferenceName("origin", name), true)
	if err != nil {
		// Try local branch.
		ref, err = repo.Reference(plumbing.NewBranchReferenceName(name), true)
		if err != nil {
			return ResolvedVersion{}, err
		}
	}

	return ResolvedVersion{
		Channel: "branch",
		Version: name,
		Hash:    ref.Hash(),
	}, nil
}

func resolveCommit(repo *git.Repository, sha string) (ResolvedVersion, error) {
	// Try full SHA first.
	hash := plumbing.NewHash(sha)
	if _, err := repo.CommitObject(hash); err == nil {
		return ResolvedVersion{
			Channel: "commit",
			Hash:    hash,
		}, nil
	}

	// Try prefix match.
	iter, err := repo.CommitObjects()
	if err != nil {
		return ResolvedVersion{}, err
	}

	var found *plumbing.Hash
	err = iter.ForEach(func(c *object.Commit) error {
		if strings.HasPrefix(c.Hash.String(), sha) {
			h := c.Hash
			found = &h
		}
		return nil
	})

	if found != nil {
		return ResolvedVersion{
			Channel: "commit",
			Hash:    *found,
		}, nil
	}

	return ResolvedVersion{}, fmt.Errorf("commit %q not found", sha)
}

func listTagNames(repo *git.Repository) ([]string, error) {
	refs, err := repo.Tags()
	if err != nil {
		return nil, err
	}

	var tags []string
	refs.ForEach(func(ref *plumbing.Reference) error {
		tags = append(tags, ref.Name().Short())
		return nil
	})

	return tags, nil
}
