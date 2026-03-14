package simulator

import (
	"bufio"
	"strings"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func scannerFromString(s string) *bufio.Scanner {
	return bufio.NewScanner(strings.NewReader(s))
}

func TestParseScript_ValidCommands(t *testing.T) {
	input := `wait 2s
key ENTER press
key ENTER release
wait 500ms
screenshot /tmp/test.png`

	commands, err := ParseScriptReader(scannerFromString(input))
	require.NoError(t, err)
	require.Len(t, commands, 5)

	assert.Equal(t, ScriptWait, commands[0].Type)
	assert.Equal(t, 2*time.Second, commands[0].Duration)

	assert.Equal(t, ScriptKeyPress, commands[1].Type)
	assert.Equal(t, "ENTER", commands[1].KeyName)

	assert.Equal(t, ScriptKeyRelease, commands[2].Type)
	assert.Equal(t, "ENTER", commands[2].KeyName)

	assert.Equal(t, ScriptWait, commands[3].Type)
	assert.Equal(t, 500*time.Millisecond, commands[3].Duration)

	assert.Equal(t, ScriptScreenshot, commands[4].Type)
	assert.Equal(t, "/tmp/test.png", commands[4].Path)
}

func TestParseScript_CommentsAndBlankLines(t *testing.T) {
	input := `# This is a comment
wait 1s

# Another comment

key EXIT press`

	commands, err := ParseScriptReader(scannerFromString(input))
	require.NoError(t, err)
	require.Len(t, commands, 2)

	assert.Equal(t, ScriptWait, commands[0].Type)
	assert.Equal(t, ScriptKeyPress, commands[1].Type)
}

func TestParseScript_InvalidCommand(t *testing.T) {
	input := `unknown_command arg1`
	_, err := ParseScriptReader(scannerFromString(input))
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "unknown command")
}

func TestParseScript_InvalidDuration(t *testing.T) {
	input := `wait notaduration`
	_, err := ParseScriptReader(scannerFromString(input))
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "invalid duration")
}

func TestParseScript_InvalidKeyAction(t *testing.T) {
	input := `key ENTER toggle`
	_, err := ParseScriptReader(scannerFromString(input))
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "press or release")
}

func TestParseScript_MissingArgs(t *testing.T) {
	tests := []struct {
		name  string
		input string
	}{
		{"wait missing arg", "wait"},
		{"key missing args", "key ENTER"},
		{"screenshot missing arg", "screenshot"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := ParseScriptReader(scannerFromString(tt.input))
			assert.Error(t, err)
		})
	}
}

func TestParseScript_Empty(t *testing.T) {
	commands, err := ParseScriptReader(scannerFromString(""))
	require.NoError(t, err)
	assert.Empty(t, commands)
}

func TestParseScript_DurationFormats(t *testing.T) {
	tests := []struct {
		input    string
		expected time.Duration
	}{
		{"wait 1s", time.Second},
		{"wait 500ms", 500 * time.Millisecond},
		{"wait 2m", 2 * time.Minute},
		{"wait 1m30s", 90 * time.Second},
	}
	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			commands, err := ParseScriptReader(scannerFromString(tt.input))
			require.NoError(t, err)
			require.Len(t, commands, 1)
			assert.Equal(t, tt.expected, commands[0].Duration)
		})
	}
}

func TestParseScript_KeyNameUppercase(t *testing.T) {
	input := `key enter press`
	commands, err := ParseScriptReader(scannerFromString(input))
	require.NoError(t, err)
	assert.Equal(t, "ENTER", commands[0].KeyName)
}

func TestScriptKeyIndex(t *testing.T) {
	tests := []struct {
		name     string
		expected int
		ok       bool
	}{
		{"ENTER", 2, true},
		{"EXIT", 1, true},
		{"SYS", 13, true},
		{"UNKNOWN", 0, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			idx, ok := ScriptKeyIndex(tt.name)
			assert.Equal(t, tt.ok, ok)
			if ok {
				assert.Equal(t, tt.expected, idx)
			}
		})
	}
}
