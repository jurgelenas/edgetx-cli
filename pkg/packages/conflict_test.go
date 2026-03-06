package packages

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestCheckConflicts_NoConflict(t *testing.T) {
	state := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS/A"}},
		},
	}

	err := CheckConflicts(state, []string{"WIDGETS/C"}, "")
	assert.NoError(t, err)
}

func TestCheckConflicts_ExactMatch(t *testing.T) {
	state := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS/MyTool"}},
		},
	}

	err := CheckConflicts(state, []string{"SCRIPTS/TOOLS/MyTool"}, "")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "A/B")
}

func TestCheckConflicts_PrefixOverlap(t *testing.T) {
	state := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS/MyTool"}},
		},
	}

	err := CheckConflicts(state, []string{"SCRIPTS/TOOLS"}, "")
	assert.Error(t, err)
}

func TestCheckConflicts_NoFalsePositive(t *testing.T) {
	state := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS"}},
		},
	}

	err := CheckConflicts(state, []string{"SCRIPTS/TOOLSET"}, "")
	assert.NoError(t, err, "SCRIPTS/TOOLS should not conflict with SCRIPTS/TOOLSET")
}

func TestCheckConflicts_SkipSelf(t *testing.T) {
	state := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS/MyTool"}},
		},
	}

	err := CheckConflicts(state, []string{"SCRIPTS/TOOLS/MyTool"}, "A/B")
	assert.NoError(t, err, "should skip self during update")
}

func TestCheckConflicts_MultipleConflicts(t *testing.T) {
	state := &State{
		Packages: []InstalledPackage{
			{Source: "A/B", Paths: []string{"SCRIPTS/TOOLS/A"}},
			{Source: "C/D", Paths: []string{"WIDGETS/C"}},
		},
	}

	err := CheckConflicts(state, []string{"SCRIPTS/TOOLS/A", "WIDGETS/C"}, "")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "A/B")
	assert.Contains(t, err.Error(), "C/D")
}
