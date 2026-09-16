local child = MiniTest.new_child_neovim()
local eq = MiniTest.expect.equality

local T = MiniTest.new_set({
  hooks = {
    pre_case = function()
      child.restart({ '-u', 'scripts/minimal_init.lua' })
      child.lua([[
        _G.opened = {}
        vim.ui.open = function(url)
          table.insert(_G.opened, url)
        end
        _G.notified = {}
        vim.notify = function(message, level)
          table.insert(_G.notified, { message = message, level = level })
        end
      ]])
    end,
    post_once = child.stop,
  },
})

T['arc ready'] = MiniTest.new_set()

T['arc ready']['opens the page on the announced port'] = function()
  child.lua([[require('cargo-arc').handle_line('arc ready 0.5.0 4321')]])
  eq(child.lua_get('_G.opened'), { 'http://127.0.0.1:4321/' })
end

--- A file of `lines` numbered lines, so a cursor position is checkable
--- against its content.
local function numbered_file(lines)
  local path = vim.fn.tempname() .. '.rs'
  local content = {}
  for i = 1, lines do
    content[i] = 'line ' .. i
  end
  vim.fn.writefile(content, path)
  return path
end

T['arc jump'] = MiniTest.new_set()

T['arc jump']['opens the file in the current window at the line'] = function()
  local path = numbered_file(100)
  child.lua(([[require('cargo-arc').handle_line('arc jump 42 %s')]]):format(path))
  eq(child.api.nvim_buf_get_name(0), path)
  eq(child.api.nvim_win_get_cursor(0), { 42, 0 })
end

T['arc jump']['uses the window that already shows the file'] = function()
  local path = numbered_file(100)
  child.cmd('edit ' .. path)
  local showing = child.api.nvim_get_current_win()
  child.cmd('new')
  local other = child.api.nvim_get_current_win()
  child.lua(([[require('cargo-arc').handle_line('arc jump 42 %s')]]):format(path))
  eq(child.api.nvim_get_current_win(), showing)
  eq(child.api.nvim_win_get_cursor(showing), { 42, 0 })
  eq(child.api.nvim_buf_get_name(child.api.nvim_win_get_buf(other)), '')
end

T['arc jump']['leaves the scroll alone when the line is visible'] = function()
  local path = numbered_file(100)
  child.cmd('edit ' .. path)
  child.lua(([[require('cargo-arc').handle_line('arc jump 5 %s')]]):format(path))
  eq(child.api.nvim_win_get_cursor(0), { 5, 0 })
  eq(child.fn.line('w0'), 1)
end

T['arc jump']['centers a line that was not visible'] = function()
  local path = numbered_file(100)
  child.cmd('edit ' .. path)
  local last_visible = child.fn.line('w$')
  local target = last_visible + 1
  child.lua(([[require('cargo-arc').handle_line('arc jump %d %s')]]):format(target, path))
  eq(child.api.nvim_win_get_cursor(0), { target, 0 })
  -- Minimal scrolling would make the target the last visible line.
  eq(child.fn.line('w$') > target, true)
  eq(child.fn.line('w0') < target, true)
end

T['arc jump']['clamps a line beyond the end of the file'] = function()
  local path = numbered_file(10)
  child.lua(([[require('cargo-arc').handle_line('arc jump 50 %s')]]):format(path))
  eq(child.api.nvim_win_get_cursor(0), { 10, 0 })
end

T['argv'] = MiniTest.new_set()

T['argv']['runs the ui subcommand of cargo-arc from PATH by default'] = function()
  eq(child.lua_get('require("cargo-arc").argv()'), { 'cargo-arc', 'arc', 'ui' })
end

T['argv']['puts the analysis options before the subcommand as their flags'] = function()
  child.lua([[require('cargo-arc').setup({
    binary = '/opt/cargo-arc',
    externals = true,
    include_tests = true,
    features = { 'hir', 'extra' },
    manifest_path = 'member/Cargo.toml',
  })]])
  eq(child.lua_get('require("cargo-arc").argv()'), {
    '/opt/cargo-arc',
    'arc',
    '--manifest-path',
    'member/Cargo.toml',
    '--features',
    'hir,extra',
    '--include-tests',
    '--externals',
    'ui',
  })
end

T['argv']['passes a port to the ui subcommand'] = function()
  eq(child.lua_get('require("cargo-arc").argv(4321)'), { 'cargo-arc', 'arc', 'ui', '--port', '4321' })
end

local fake_service = vim.fn.fnamemodify('tests/fake_service.lua', ':p')

--- Fails unless `condition` becomes true within a few seconds.
local function wait_for(condition)
  eq(vim.wait(5000, condition, 20), true)
end

T['service'] = MiniTest.new_set({
  hooks = {
    pre_case = function()
      child.lua(([[require('cargo-arc').setup({ binary = '%s' })]]):format(fake_service))
    end,
  },
})

--- Starts the fake service in the child and returns its status once ready.
local function open_and_wait()
  child.lua([[require('cargo-arc').open()]])
  wait_for(function()
    return child.lua_get('require("cargo-arc").status()') ~= vim.NIL
  end)
  return child.lua_get('require("cargo-arc").status()')
end

T['service']['open starts the binary and opens the announced page'] = function()
  local status = open_and_wait()
  wait_for(function()
    return #child.lua_get('_G.opened') == 1
  end)
  eq(child.lua_get('_G.opened'), { 'http://127.0.0.1:' .. status.port .. '/' })
end

T['service']['a second open reopens the page without a second process'] = function()
  local first = open_and_wait()
  child.lua([[require('cargo-arc').open()]])
  wait_for(function()
    return #child.lua_get('_G.opened') == 2
  end)
  eq(child.lua_get('require("cargo-arc").status()'), first)
end

--- Whether a process with `pid` still exists, seen from the test runner.
local function alive(pid)
  return vim.uv.kill(pid, 0) == 0
end

T['service']['stop ends the process'] = function()
  local pid = open_and_wait().pid
  eq(alive(pid), true)
  child.lua([[require('cargo-arc').stop()]])
  wait_for(function()
    return child.lua_get('require("cargo-arc").status()') == vim.NIL
  end)
  wait_for(function()
    return not alive(pid)
  end)
end

T['service']['restart replaces the process on the same port and keeps the page'] = function()
  local first = open_and_wait()
  child.lua([[require('cargo-arc').restart()]])
  local second
  wait_for(function()
    second = child.lua_get('require("cargo-arc").status()')
    return second ~= vim.NIL and second.pid ~= first.pid
  end)
  -- The fake picks its port from its pid unless one is passed, so the same
  -- port from a new process proves the plugin passed it.
  eq(second.port, first.port)
  eq(alive(first.pid), false)
  eq(#child.lua_get('_G.opened'), 1)
  local notified = child.lua_get('_G.notified')
  eq(#notified, 1)
  eq(notified[1].level, child.lua_get('vim.log.levels.INFO'))
  eq(notified[1].message:find(tostring(first.port), 1, true) ~= nil, true)
end

T['service']['a missing binary is reported with its path'] = function()
  child.lua([[require('cargo-arc').setup({ binary = '/nonexistent/cargo-arc' })]])
  child.lua([[require('cargo-arc').open()]])
  local notified = child.lua_get('_G.notified')
  eq(#notified, 1)
  eq(notified[1].level, child.lua_get('vim.log.levels.ERROR'))
  eq(notified[1].message:find('/nonexistent/cargo-arc', 1, true) ~= nil, true)
  eq(child.lua_get('require("cargo-arc").status()'), vim.NIL)
end

T['service']['an exit before the ready line is reported with code and stderr'] = function()
  child.lua([[vim.env.ARC_FAKE_FAIL = '1']])
  child.lua([[require('cargo-arc').open()]])
  wait_for(function()
    return #child.lua_get('_G.notified') == 1
  end)
  local notice = child.lua_get('_G.notified')[1]
  eq(notice.level, child.lua_get('vim.log.levels.ERROR'))
  eq(notice.message:find('exit code 2', 1, true) ~= nil, true)
  eq(notice.message:find('workspace has no crates', 1, true) ~= nil, true)
  eq(child.lua_get('_G.opened'), {})
end

T['service'][':Arc open, stop and restart drive the service'] = function()
  child.cmd('Arc open')
  local first = child.lua_get('require("cargo-arc").status()')
  wait_for(function()
    first = child.lua_get('require("cargo-arc").status()')
    return first ~= vim.NIL
  end)
  child.cmd('Arc restart')
  wait_for(function()
    local status = child.lua_get('require("cargo-arc").status()')
    return status ~= vim.NIL and status.pid ~= first.pid
  end)
  child.cmd('Arc stop')
  wait_for(function()
    return child.lua_get('require("cargo-arc").status()') == vim.NIL
  end)
end

T['service'][':Arc completes its subcommands and refuses others'] = function()
  eq(child.fn.getcompletion('Arc ', 'cmdline'), { 'open', 'restart', 'stop', 'follow' })
  eq(child.fn.getcompletion('Arc re', 'cmdline'), { 'restart' })
  eq(child.fn.getcompletion('Arc follow ', 'cmdline'), { 'on', 'off' })
  child.cmd('Arc bogus')
  child.cmd('Arc open extra')
  local notified = child.lua_get('_G.notified')
  eq(#notified, 2)
  eq(notified[1].level, child.lua_get('vim.log.levels.ERROR'))
  eq(notified[1].message:find('bogus', 1, true) ~= nil, true)
  eq(notified[2].level, child.lua_get('vim.log.levels.ERROR'))
  eq(notified[2].message:find('open', 1, true) ~= nil, true)
  eq(child.lua_get('require("cargo-arc").status()'), vim.NIL)
end

--- The lines the fake has received on stdin so far, once at least
--- `count` have arrived.
--- @param log string
--- @param count integer
--- @return string[]
local function stdin_lines(log, count)
  local lines
  wait_for(function()
    lines = vim.fn.filereadable(log) == 1 and vim.fn.readfile(log) or {}
    return #lines >= count
  end)
  return lines
end

T['arc focus'] = MiniTest.new_set({
  hooks = {
    pre_case = function()
      local log = vim.fn.tempname()
      child.lua(([[vim.env.ARC_FAKE_STDIN_LOG = '%s']]):format(log))
      child.lua(([[require('cargo-arc').setup({ binary = '%s' })]]):format(fake_service))
      _G.stdin_log = log
    end,
  },
})

T['arc focus']['entering a file buffer writes its line and absolute path'] = function()
  open_and_wait()
  local path = numbered_file(20)
  child.cmd('edit ' .. path)
  child.api.nvim_win_set_cursor(0, { 7, 0 })
  child.cmd('new')
  child.cmd('wincmd p')
  local lines = stdin_lines(_G.stdin_log, 2)
  eq(lines[#lines], 'arc focus 7 ' .. path)
end

T['arc focus']['a buffer without a name writes nothing'] = function()
  open_and_wait()
  child.cmd('enew')
  child.cmd('new')
  child.cmd('wincmd p')
  vim.wait(300)
  eq(vim.fn.filereadable(_G.stdin_log), 0)
end

T['arc focus']['a buffer with a buftype writes nothing'] = function()
  open_and_wait()
  child.cmd('help')
  vim.wait(300)
  eq(vim.fn.filereadable(_G.stdin_log), 0)
end

T['arc focus'][':Arc follow off and on write the follow lines'] = function()
  open_and_wait()
  child.cmd('Arc follow off')
  eq(stdin_lines(_G.stdin_log, 1)[1], 'arc follow off')
  child.cmd('Arc follow on')
  eq(stdin_lines(_G.stdin_log, 2)[2], 'arc follow on')
end

T['arc focus'][':Arc follow with another argument reports and writes nothing'] = function()
  open_and_wait()
  child.cmd('Arc follow maybe')
  local notified = child.lua_get('_G.notified')
  eq(#notified, 1)
  eq(notified[1].level, child.lua_get('vim.log.levels.ERROR'))
  eq(notified[1].message:find('maybe', 1, true) ~= nil, true)
  vim.wait(300)
  eq(vim.fn.filereadable(_G.stdin_log), 0)
end

T['arc focus'][':Arc follow without a service reports it'] = function()
  child.cmd('Arc follow off')
  local notified = child.lua_get('_G.notified')
  eq(#notified, 1)
  eq(notified[1].level, child.lua_get('vim.log.levels.INFO'))
end

T['service']['leaving Neovim ends the process'] = function()
  local pid = open_and_wait().pid
  child.stop()
  wait_for(function()
    return not alive(pid)
  end)
end

--- Sends one request to the service and lets the response go unread; the
--- effect under test arrives on the service's stdout.
local function http_get(port, path)
  local socket = vim.uv.new_tcp()
  socket:connect('127.0.0.1', port, function(err)
    assert(not err, err)
    socket:write('GET ' .. path .. ' HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n')
    socket:read_start(function(_, data)
      if not data then
        socket:close()
      end
    end)
  end)
end

local repo = vim.fn.fnamemodify('../..', ':p'):gsub('/$', '')

T['end to end'] = MiniTest.new_set()

T['end to end']['a jump request against the real service opens the file'] = function()
  local fixture = repo .. '/tests/fixtures/multi_crate'
  child.cmd('cd ' .. fixture)
  child.lua(([[require('cargo-arc').setup({ binary = '%s/target/release/cargo-arc' })]]):format(repo))
  local status = open_and_wait()
  local reported = vim.trim(vim.fn.system({ repo .. '/target/release/cargo-arc', 'arc', '--version' }))
  eq(reported:match('%S+$'), status.version)
  eq(child.lua_get('_G.opened'), { 'http://127.0.0.1:' .. status.port .. '/' })
  http_get(status.port, '/jump?id=0')
  wait_for(function()
    return child.api.nvim_buf_get_name(0) ~= ''
  end)
  eq(vim.startswith(child.api.nvim_buf_get_name(0), fixture .. '/'), true)
end

return T
