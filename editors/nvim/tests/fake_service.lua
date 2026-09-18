#!/usr/bin/env -S nvim -l
-- Stands in for `cargo-arc arc ui`: announces a port and the analysis
-- switches from its flags, answers a switch line on stdin with the new
-- state, and stays alive until killed, or fails before announcing when
-- ARC_FAKE_FAIL is set. The port is the one after `--port`, otherwise one
-- derived from the pid, so two runs without the flag announce different
-- ports. With ARC_FAKE_STDIN_LOG set, every line received on stdin is
-- appended to that file.
if os.getenv('ARC_FAKE_FAIL') then
  io.stderr:write('workspace has no crates to determine its root\n')
  os.exit(2)
end
local port
local switches = { externals = false, tests = false }
for i, value in ipairs(_G.arg) do
  if value == '--port' then
    port = _G.arg[i + 1]
  elseif value == '--externals' then
    switches.externals = true
  elseif value == '--include-tests' then
    switches.tests = true
  end
end
port = port or (10000 + vim.uv.os_getpid() % 50000)

local function word(on)
  return on and 'on' or 'off'
end

local function announce_switches()
  io.stdout:write('arc analysis externals=' .. word(switches.externals) .. ' tests=' .. word(switches.tests) .. '\n')
  io.stdout:flush()
end

io.stdout:write('arc ready 0.0.0 ' .. port .. '\n')
io.stdout:flush()
announce_switches()

local log = os.getenv('ARC_FAKE_STDIN_LOG')

--- Logs the line when asked to, and answers a switch line like the
--- service: `arc externals on` reports the state once "recomputed".
--- @param line string
local function handle_line(line)
  if log then
    local file = assert(io.open(log, 'a'))
    file:write(line .. '\n')
    file:close()
  end
  local name, state = line:match('^arc (%a+) (%a+)$')
  if (name == 'externals' or name == 'tests') and (state == 'on' or state == 'off') then
    switches[name] = state == 'on'
    announce_switches()
  end
end

-- Read through libuv, not io.lines(): a blocking read would keep the
-- signal handler from running, and the plugin's kill would hang.
local stdin = vim.uv.new_pipe()
stdin:open(0)
local pending = ''
stdin:read_start(function(_, data)
  if not data then
    return
  end
  pending = pending .. data
  while true do
    local newline = pending:find('\n', 1, true)
    if not newline then
      break
    end
    handle_line(pending:sub(1, newline - 1))
    pending = pending:sub(newline + 1)
  end
end)
while true do
  vim.wait(1000)
end
