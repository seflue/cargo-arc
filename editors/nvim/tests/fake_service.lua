#!/usr/bin/env -S nvim -l
-- Stands in for `cargo-arc arc ui`: announces a port and stays alive until
-- killed, or fails before announcing when ARC_FAKE_FAIL is set. The port is
-- the one after `--port`, otherwise one derived from the pid, so two runs
-- without the flag announce different ports. With ARC_FAKE_STDIN_LOG set,
-- every line received on stdin is appended to that file.
if os.getenv('ARC_FAKE_FAIL') then
  io.stderr:write('workspace has no crates to determine its root\n')
  os.exit(2)
end
local port
for i, value in ipairs(_G.arg) do
  if value == '--port' then
    port = _G.arg[i + 1]
  end
end
port = port or (10000 + vim.uv.os_getpid() % 50000)
io.stdout:write('arc ready 0.0.0 ' .. port .. '\n')
io.stdout:flush()
local log = os.getenv('ARC_FAKE_STDIN_LOG')
if log then
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
      local file = assert(io.open(log, 'a'))
      file:write(pending:sub(1, newline))
      file:close()
      pending = pending:sub(newline + 1)
    end
  end)
end
while true do
  vim.wait(1000)
end
