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
