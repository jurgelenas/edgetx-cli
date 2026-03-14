package simulator

import (
	"bufio"
	"context"
	"fmt"
	"image"
	"image/png"
	"os"
	"strings"
	"time"
)

// ScriptCommandType identifies the type of action script command.
type ScriptCommandType int

const (
	ScriptWait ScriptCommandType = iota
	ScriptKeyPress
	ScriptKeyRelease
	ScriptScreenshot
)

// ScriptCommand represents a single action script command.
type ScriptCommand struct {
	Type     ScriptCommandType
	Duration time.Duration // for Wait
	KeyName  string        // for KeyPress/KeyRelease
	Path     string        // for Screenshot
}

// ParseScript reads an action script file and returns the command list.
func ParseScript(path string) ([]ScriptCommand, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("opening script: %w", err)
	}
	defer f.Close()

	return ParseScriptReader(bufio.NewScanner(f))
}

// ParseScriptReader parses script commands from a scanner.
func ParseScriptReader(scanner *bufio.Scanner) ([]ScriptCommand, error) {
	var commands []ScriptCommand
	lineNum := 0

	for scanner.Scan() {
		lineNum++
		line := strings.TrimSpace(scanner.Text())

		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		parts := strings.Fields(line)
		if len(parts) == 0 {
			continue
		}

		switch parts[0] {
		case "wait":
			if len(parts) != 2 {
				return nil, fmt.Errorf("line %d: wait requires a duration argument", lineNum)
			}
			d, err := time.ParseDuration(parts[1])
			if err != nil {
				return nil, fmt.Errorf("line %d: invalid duration %q: %w", lineNum, parts[1], err)
			}
			commands = append(commands, ScriptCommand{Type: ScriptWait, Duration: d})

		case "key":
			if len(parts) != 3 {
				return nil, fmt.Errorf("line %d: key requires <name> press|release", lineNum)
			}
			keyName := strings.ToUpper(parts[1])
			action := strings.ToLower(parts[2])
			switch action {
			case "press":
				commands = append(commands, ScriptCommand{Type: ScriptKeyPress, KeyName: keyName})
			case "release":
				commands = append(commands, ScriptCommand{Type: ScriptKeyRelease, KeyName: keyName})
			default:
				return nil, fmt.Errorf("line %d: key action must be press or release, got %q", lineNum, action)
			}

		case "screenshot":
			if len(parts) != 2 {
				return nil, fmt.Errorf("line %d: screenshot requires a file path", lineNum)
			}
			commands = append(commands, ScriptCommand{Type: ScriptScreenshot, Path: parts[1]})

		default:
			return nil, fmt.Errorf("line %d: unknown command %q", lineNum, parts[0])
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("reading script: %w", err)
	}

	return commands, nil
}

// ExecuteScript runs the parsed commands against a simulator runtime.
func ExecuteScript(ctx context.Context, commands []ScriptCommand, rt *Runtime, getLCD func() []byte, display DisplayDef) error {
	for i, cmd := range commands {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		switch cmd.Type {
		case ScriptWait:
			select {
			case <-time.After(cmd.Duration):
			case <-ctx.Done():
				return ctx.Err()
			}

		case ScriptKeyPress:
			idx, ok := ScriptKeyIndex(cmd.KeyName)
			if !ok {
				return fmt.Errorf("command %d: unknown key %q", i+1, cmd.KeyName)
			}
			rt.SetKey(idx, true)

		case ScriptKeyRelease:
			idx, ok := ScriptKeyIndex(cmd.KeyName)
			if !ok {
				return fmt.Errorf("command %d: unknown key %q", i+1, cmd.KeyName)
			}
			rt.SetKey(idx, false)

		case ScriptScreenshot:
			lcd := getLCD()
			if lcd == nil {
				return fmt.Errorf("command %d: no LCD data available", i+1)
			}
			rgba := DecodeFramebuffer(lcd, display)
			if err := saveScreenshot(cmd.Path, rgba, display.W, display.H); err != nil {
				return fmt.Errorf("command %d: %w", i+1, err)
			}
		}
	}
	return nil
}

// ScriptKeyIndex maps a key name to its simulator index.
func ScriptKeyIndex(name string) (int, bool) {
	idx, ok := scriptKeyMap[name]
	return idx, ok
}

var scriptKeyMap = map[string]int{
	"MENU":   0,
	"EXIT":   1,
	"ENTER":  2,
	"PAGEUP": 3,
	"PAGEDN": 4,
	"UP":     5,
	"DOWN":   6,
	"LEFT":   7,
	"RIGHT":  8,
	"PLUS":   9,
	"MINUS":  10,
	"MODEL":  11,
	"TELE":   12,
	"SYS":    13,
}

func saveScreenshot(path string, rgba []byte, w, h int) error {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	copy(img.Pix, rgba)

	f, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("creating screenshot file: %w", err)
	}
	defer f.Close()

	if err := png.Encode(f, img); err != nil {
		return fmt.Errorf("encoding PNG: %w", err)
	}
	return f.Close()
}
