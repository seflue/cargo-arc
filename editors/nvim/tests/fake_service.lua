#!/usr/bin/env -S nvim -l
-- Stands in for `cargo-arc arc ui`: announces a port and stays alive until
-- killed, or fails before announcing when ARC_FAKE_FAIL is set. The port is
-- the one after `--port`, otherwise one derived from the pid, so two runs
-- without the flag announce different ports.
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
while true do
  vim.wait(1000)
end
