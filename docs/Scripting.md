# Lua Test Scripts

Test scripts are written in Lua 5.4 and automate simulator interaction for testing. Pass a script file with `--script`:

```sh
edgetx-cli dev simulator --radio "Radiomaster TX16S" --headless --script test.lua --timeout 30s --screenshot result.png
```

## Key commands

| Function              | Description                                |
|-----------------------|--------------------------------------------|
| `key.press(key)`      | Tap: key down, 100ms pause, key up         |
| `key.longpress(key)`  | Long press: key down, 1s pause, key up     |
| `key.down(key)`       | Hold a key down                            |
| `key.up(key)`         | Release a key                              |

Key arguments accept a `KEY` constant or a string: `key.press(KEY.ENTER)` or `key.press("ENTER")`.

**KEY constants:** `KEY.MENU`, `KEY.EXIT`, `KEY.ENTER`, `KEY.PAGEUP`, `KEY.PAGEDN`, `KEY.UP`, `KEY.DOWN`, `KEY.LEFT`, `KEY.RIGHT`, `KEY.PLUS`, `KEY.MINUS`, `KEY.MODEL`, `KEY.TELE`, `KEY.SYS`

## Touch commands

| Function                  | Description                                |
|---------------------------|---------------------------------------------|
| `touch.tap(x, y)`        | Tap: touch down, 100ms pause, touch up     |
| `touch.longpress(x, y)`  | Long press: down, 1s pause, up             |
| `touch.down(x, y)`       | Hold touch at coordinates                  |
| `touch.release()`        | Release touch                              |

## Trim commands

| Function                  | Description                                |
|---------------------------|--------------------------------------------|
| `trim.press(name)`       | Tap: down, 100ms pause, up                |
| `trim.longpress(name)`   | Long press: down, 1s pause, up            |
| `trim.down(name)`        | Hold trim button                           |
| `trim.up(name)`          | Release trim button                        |
| `trim.get(name)`         | Get current trim value                     |
| `trim.set(name, value)`  | Set trim to an exact value                 |
| `trim.range()`           | Get trim min/max limits (returns two values)|

Trim arguments accept a `TRIM` constant, string name, or raw index: `trim.press(TRIM.T1)` or `trim.press("T1")`.

## Hardware inputs

| Function                    | Description                                      |
|-----------------------------|--------------------------------------------------|
| `switch(name, state)`       | Set switch position (`-1`, `0`, `1`)             |
| `analog(name, value)`       | Set analog input (`0`-`4096`)                    |
| `rotary(delta)`             | Rotary encoder delta                             |

Switch and analog accept a `SWITCH`/`INPUT` constant, string name, or raw index: `switch(SWITCH.SA, -1)` or `switch("SA", -1)`.

**SWITCH / INPUT / TRIM constants** are radio-specific and auto-populated from the radio definition (e.g., `SWITCH.SA`, `SWITCH.SB`, `INPUT.LH`, `INPUT.P1`, `TRIM.T1`, `TRIM.T4`).

## Monitor commands

### Channel outputs

| Function              | Description                                          |
|-----------------------|------------------------------------------------------|
| `channel.get(index)`  | Get channel output value (1-based, -1024 to 1024)    |
| `channel.mixer(index)`| Get mixer output value (1-based)                     |
| `channel.count()`     | Number of output channels                            |
| `channel.used(index)` | Whether channel is in use (boolean, 1-based)         |
| `channel.mix_count()` | Number of active mixes in the model                  |

### Logical switches

| Function                    | Description                                    |
|-----------------------------|------------------------------------------------|
| `logicalswitch.get(index)`  | Get logical switch state (true/false, 1-based) |
| `logicalswitch.count()`     | Number of logical switches                     |

### Global variables

| Function                        | Description                          |
|---------------------------------|--------------------------------------|
| `gvar.get(gvar, flightmode)`    | Get GVar value (both 1-based)        |
| `gvar.count()`                  | Number of global variables           |
| `gvar.flightmodes()`           | Number of flight modes               |
| `gvar.flightmode()`            | Current active flight mode (0-based) |

Example:

```lua
-- Read all active logical switches
for i = 1, logicalswitch.count() do
    if logicalswitch.get(i) then
        print("L" .. i .. " is ON")
    end
end

-- Read channel outputs
for i = 1, channel.count() do
    if channel.used(i) then
        print("CH" .. i .. " = " .. channel.get(i))
    end
end
```

## Utilities

| Function              | Description                                |
|-----------------------|--------------------------------------------|
| `wait(seconds)`       | Wait for a duration (float, in seconds)    |
| `screenshot(path)`    | Save LCD framebuffer as PNG                |
| `reset()`             | Full simulator restart — reloads all scripts, widgets, and resets screen |
| `reload()`            | Reload Lua scripts from SD card (mix, function, telemetry — not widgets) |
| `exit(code)`          | Exit with a process exit code              |
| `print(...)`          | Debug logging (Lua standard library)       |

## Exit codes

Scripts return exit code 0 by default. Use `exit(code)` to terminate early with a specific exit code. This is useful for CI pipelines where you need to signal pass/fail.

## Error handling

Scripts halt immediately on any error. Error messages include file name, line number, and a description of the problem (e.g., `unknown key "BOGUS" (available: MENU, EXIT, ...)`). Script errors produce a non-zero exit code.

## Example script

`test.lua`:

```lua
-- Wait for boot
wait(5)

-- Navigate to the tools menu
key.press(KEY.SYS)
wait(1)
key.press(KEY.PAGEDN)
wait(0.5)

-- Take a screenshot
screenshot("tools-menu.png")
```

## CI example

```sh
edgetx-cli dev simulator \
  --radio "Radiomaster TX16S" \
  --headless \
  --script test.lua \
  --timeout 30s \
  --screenshot final.png
```

## Stdin streaming

Use `--script -` or `--script-stdin` to read Lua commands from stdin. This enables AI-driven testing and interactive piped scripting. Multi-line constructs (e.g., `for`/`end` blocks) are automatically detected and buffered until complete.

```sh
# Pipe commands
echo 'print("hello")' | edgetx-cli dev simulator \
  --radio "Radiomaster TX16S" --headless --script - --timeout 10s

# Multi-line via stdin
printf 'for i=1,3 do\nprint(i)\nend\n' | edgetx-cli dev simulator \
  --radio "Radiomaster TX16S" --headless --script-stdin --timeout 10s

# Exit with a specific code
echo 'exit(42)' | edgetx-cli dev simulator \
  --radio "Radiomaster TX16S" --headless --script - --timeout 10s
echo $?  # prints 42
```

## Interactive Lua scripting via stdin

When an AI agent (e.g. Claude Code) needs to interactively control the simulator — sending commands one at a time, observing the screen, and deciding what to do next — a simple pipe won't work because each shell invocation is a separate process. Instead, use `tail -f` on a regular file to keep the simulator's stdin open across multiple shell calls.

### Setup — launch the simulator with `tail -f`

```sh
# Create a command file and log file
touch /tmp/sim-cmds
touch /tmp/sim-log

# Launch the simulator in the background, feeding commands via tail -f
tail -f /tmp/sim-cmds | edgetx-cli dev simulator \
  --radio "Radiomaster TX16S" \
  --headless \
  --script-stdin \
  --timeout 120s \
  > /tmp/sim-log 2>&1 &
SIM_PID=$!
```

The key insight: each `echo 'cmd' >> /tmp/sim-cmds` is a separate shell invocation that appends to a regular file. `tail -f` watches that file for new content and continuously feeds new lines into the simulator's stdin — bridging separate shell calls into one persistent stream.

### Boot wait — the simulator needs ~3 seconds to fully start

```sh
# Always wait for the simulator to boot before sending navigation commands
echo 'wait(3)' >> /tmp/sim-cmds
```

### Observe-act loop — the core interaction pattern

1. Send Lua commands by appending to the command file (see scripting API above)
2. Take a screenshot to observe the result
3. Read the screenshot PNG to see what happened
4. Decide the next action and repeat

```sh
# Send a command
echo 'key.press(KEY.SYS)' >> /tmp/sim-cmds

# Wait for the UI to update, then capture a screenshot
echo 'wait(1)' >> /tmp/sim-cmds
echo 'screenshot("/tmp/sim-screen.png")' >> /tmp/sim-cmds

# Give the screenshot time to be written, then read the PNG to observe the result
sleep 1
# (AI agent reads /tmp/sim-screen.png to see the current screen)

# Send the next command based on what was observed
echo 'touch.tap(240, 20)' >> /tmp/sim-cmds
```

`print()` output goes to stdout, which is captured in the log file (`/tmp/sim-log`). Read the log file to see Lua print output.

For multi-line Lua blocks (e.g. `for`/`end`), use a heredoc so all lines arrive together for the block detector:

```sh
cat >> /tmp/sim-cmds << 'CMDS'
for i = 1, 5 do
    rotary(1)
    wait(0.3)
end
CMDS
```

### Exit code — signal pass/fail to the calling process

```sh
# Tell the simulator to exit with a specific code
echo 'exit(0)' >> /tmp/sim-cmds

# Wait for the process to finish and capture its exit code
wait $SIM_PID
echo $?  # prints 0
```

### Cleanup — always clean up when done

```sh
# Kill the simulator if still running
kill $SIM_PID 2>/dev/null
wait $SIM_PID 2>/dev/null

# Remove temp files
rm -f /tmp/sim-cmds /tmp/sim-log /tmp/sim-screen.png
```

### Advanced example (loops, functions)

```lua
-- Helper to navigate down N times
function nav_down(n)
    for i = 1, n do
        key.press(KEY.DOWN)
        wait(0.2)
    end
end

wait(3)
key.press(KEY.SYS)
wait(1)
nav_down(5)
screenshot("result.png")
```

## Complete API showcase

A single script demonstrating every available Lua function and constant table.

`showcase.lua`:

```lua
-- ============================================================
-- Complete API Showcase
-- Demonstrates all 34 Lua functions and 4 constant tables
-- ============================================================

-- ---- Utilities: wait, print --------------------------------
-- Wait for the simulator to fully boot
wait(5)
print("Simulator booted")

-- ---- Key commands ------------------------------------------
-- key.press(key)     — tap: down, 100ms pause, up
-- key.longpress(key) — long press: down, 1s pause, up
-- key.down(key)      — hold a key down
-- key.up(key)        — release a key

key.press(KEY.SYS)           -- open system menu (using KEY constant)
wait(1)
key.press("PAGEDN")          -- next page (using string name)
wait(0.5)
key.longpress(KEY.ENTER)     -- long-press enter
wait(0.5)
key.down(KEY.EXIT)            -- hold EXIT
wait(0.2)
key.up(KEY.EXIT)              -- release EXIT
wait(1)

-- ---- Touch commands ----------------------------------------
-- touch.tap(x, y)        — tap: down, 100ms pause, up
-- touch.longpress(x, y)  — long press: down, 1s pause, up
-- touch.down(x, y)       — hold touch at coordinates
-- touch.release()        — release touch

touch.tap(240, 160)           -- tap center of a 480x272 screen
wait(0.5)
touch.longpress(100, 50)      -- long press near top-left
wait(0.5)
touch.down(300, 200)          -- hold touch
wait(0.3)
touch.release()               -- release touch
wait(0.5)

-- ---- Trim commands -----------------------------------------
-- trim.press(name)       — tap: down, 100ms pause, up
-- trim.longpress(name)   — long press: down, 1s pause, up
-- trim.down(name)        — hold trim button
-- trim.up(name)          — release trim button
-- trim.get(name)         — get current trim value
-- trim.set(name, value)  — set trim to an exact value
-- trim.range()           — get trim min/max limits

local trim_min, trim_max = trim.range()
print("Trim range: " .. trim_min .. " to " .. trim_max)

trim.press(TRIM.T1)           -- tap trim T1 (using TRIM constant)
wait(0.3)
trim.longpress("T1")          -- long press trim T1 (using string name)
wait(0.3)
trim.down(TRIM.T1)            -- hold trim T1
wait(0.2)
trim.up(TRIM.T1)              -- release trim T1
wait(0.3)

local val = trim.get(TRIM.T1)
print("T1 trim value: " .. val)

trim.set(TRIM.T1, 0)          -- reset trim T1 to center
print("T1 trim reset to: " .. trim.get(TRIM.T1))
wait(0.5)

-- ---- Hardware inputs: switch, analog, rotary ---------------
-- switch(name, state) — set switch position (-1, 0, 1)
-- analog(name, value) — set analog input (0–4096)
-- rotary(delta)       — rotary encoder delta

switch(SWITCH.SA, -1)          -- switch SA up (using SWITCH constant)
wait(0.3)
switch("SA", 0)                -- switch SA middle (using string name)
wait(0.3)
switch(SWITCH.SA, 1)           -- switch SA down
wait(0.3)

analog(INPUT.LV, 2048)         -- left vertical stick to center (using INPUT constant)
wait(0.3)
analog("RH", 4096)             -- right horizontal stick to max (using string name)
wait(0.3)

rotary(3)                       -- scroll rotary encoder 3 steps forward
wait(0.3)
rotary(-2)                      -- scroll rotary encoder 2 steps back
wait(0.5)

-- ---- Monitor: channel outputs ------------------------------
-- channel.count()       — number of output channels
-- channel.get(index)    — channel output value (1-based, -1024 to 1024)
-- channel.mixer(index)  — mixer output value (1-based)
-- channel.used(index)   — whether channel is in use (1-based)
-- channel.mix_count()   — number of active mixes

print("Channel count: " .. channel.count())
print("Active mixes:  " .. channel.mix_count())

for i = 1, channel.count() do
    if channel.used(i) then
        local ch_val = channel.get(i)
        local mx_val = channel.mixer(i)
        print("CH" .. i .. "  output=" .. ch_val .. "  mixer=" .. mx_val)
    end
end

-- ---- Monitor: logical switches -----------------------------
-- logicalswitch.count()      — number of logical switches
-- logicalswitch.get(index)   — logical switch state (true/false, 1-based)

print("Logical switches: " .. logicalswitch.count())

for i = 1, logicalswitch.count() do
    if logicalswitch.get(i) then
        print("L" .. i .. " is ON")
    end
end

-- ---- Monitor: global variables -----------------------------
-- gvar.count()                    — number of global variables
-- gvar.flightmodes()             — number of flight modes
-- gvar.flightmode()              — current active flight mode (0-based)
-- gvar.get(gvar, flightmode)     — get GVar value (both 1-based)

print("GVars: " .. gvar.count() .. "  Flight modes: " .. gvar.flightmodes())
print("Current flight mode: " .. gvar.flightmode())

for g = 1, gvar.count() do
    local value = gvar.get(g, gvar.flightmode() + 1)  -- flightmode() is 0-based, get() is 1-based
    print("GV" .. g .. " = " .. value)
end

-- ---- Utilities: screenshot, reload, reset ------------------
-- screenshot(path) — save LCD framebuffer as PNG
-- reload()         — reload Lua scripts from SD card
-- reset()          — full simulator restart

screenshot("showcase-before-reload.png")
print("Screenshot saved")

reload()             -- reload mix/function/telemetry Lua scripts
wait(2)

screenshot("showcase-after-reload.png")

reset()              -- full simulator restart (reloads everything)
wait(5)

screenshot("showcase-after-reset.png")

-- ---- Exit --------------------------------------------------
-- exit(code) — exit with a process exit code

print("Showcase complete")
exit(0)
```
