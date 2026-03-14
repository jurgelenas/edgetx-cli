package radio

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestCheckVersionCompatibility(t *testing.T) {
	tests := []struct {
		name         string
		radioVersion string
		minVersion   string
		wantErr      bool
		errContains  string
	}{
		{
			name:         "equal versions",
			radioVersion: "2.12.0",
			minVersion:   "2.12.0",
		},
		{
			name:         "radio newer",
			radioVersion: "2.13.0",
			minVersion:   "2.12.0",
		},
		{
			name:         "radio older",
			radioVersion: "2.11.0",
			minVersion:   "2.12.0",
			wantErr:      true,
			errContains:  "does not meet minimum",
		},
		{
			name:         "empty min version",
			radioVersion: "2.11.0",
			minVersion:   "",
		},
		{
			name:         "with v prefix on both",
			radioVersion: "v2.13.0",
			minVersion:   "v2.12.0",
		},
		{
			name:         "v prefix on radio only",
			radioVersion: "v2.12.0",
			minVersion:   "2.12.0",
		},
		{
			name:         "v prefix on min only",
			radioVersion: "2.12.0",
			minVersion:   "v2.12.0",
		},
		{
			name:         "invalid radio version",
			radioVersion: "not-a-version",
			minVersion:   "2.12.0",
			wantErr:      true,
			errContains:  "invalid radio firmware version",
		},
		{
			name:         "invalid min version",
			radioVersion: "2.12.0",
			minVersion:   "bad",
			wantErr:      true,
			errContains:  "invalid minimum version",
		},
		{
			name:         "pre-release radio older",
			radioVersion: "2.12.0-rc1",
			minVersion:   "2.12.0",
			wantErr:      true,
			errContains:  "does not meet minimum",
		},
		{
			name:         "patch version newer",
			radioVersion: "2.12.1",
			minVersion:   "2.12.0",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := CheckVersionCompatibility(tt.radioVersion, tt.minVersion)
			if tt.wantErr {
				assert.Error(t, err)
				if tt.errContains != "" {
					assert.Contains(t, err.Error(), tt.errContains)
				}
			} else {
				assert.NoError(t, err)
			}
		})
	}
}
