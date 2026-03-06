package radio

import (
	"fmt"
	"strings"

	"golang.org/x/mod/semver"
)

// CheckVersionCompatibility returns an error if radioVersion is older than
// minVersion. Returns nil if minVersion is empty. Both versions are normalized
// with a "v" prefix before comparison.
func CheckVersionCompatibility(radioVersion, minVersion string) error {
	if minVersion == "" {
		return nil
	}

	rv := normalizeVersion(radioVersion)
	mv := normalizeVersion(minVersion)

	if !semver.IsValid(rv) {
		return fmt.Errorf("invalid radio firmware version %q", radioVersion)
	}
	if !semver.IsValid(mv) {
		return fmt.Errorf("invalid minimum version %q", minVersion)
	}

	if semver.Compare(rv, mv) < 0 {
		return fmt.Errorf("radio firmware version %s does not meet minimum required version %s", radioVersion, minVersion)
	}

	return nil
}

func normalizeVersion(v string) string {
	if !strings.HasPrefix(v, "v") {
		return "v" + v
	}
	return v
}
