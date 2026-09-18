local M = {}

local defaults = {
  --- The cargo-arc binary; a bare name is looked up in PATH.
  binary = 'cargo-arc',
  --- Manifest to analyze, relative to Neovim's working directory; nil for
  --- the service's own default, `Cargo.toml` in that directory.
  manifest_path = nil,
  --- Cargo features to activate.
  features = {},
  --- Include test code in the analysis.
  include_tests = false,
  --- Include external crate dependencies.
  externals = false,
}

M.config = vim.deepcopy(defaults)

--- @param opts table|nil overrides for the fields of `defaults`
function M.setup(opts)
  M.config = vim.tbl_deep_extend('force', defaults, opts or {})
end

--- @alias cargo-arc.Switches { externals: boolean, tests: boolean }

--- The running service: its process, once it has announced itself the
--- version and port from its ready line, the analysis switches it last
--- reported, the port of the service it replaces, and whether its exit was
--- asked for and whether a restart waits for it.
--- @type { process: vim.SystemObj, version: string|nil, port: integer|nil, switches: cargo-arc.Switches|nil, replaces: integer|nil, stopping: boolean|nil, restart: boolean|nil }|nil
local service = nil

--- Version, port and pid of a service that has announced itself, and the
--- switches it last reported; nil before that and while none runs.
--- @return { version: string, port: integer, pid: integer, switches: cargo-arc.Switches|nil }|nil
function M.status()
  if not service or not service.port then
    return nil
  end
  return { version = service.version, port = service.port, pid = service.process.pid, switches = service.switches }
end

--- @param port integer
local function open_page(port)
  vim.ui.open('http://127.0.0.1:' .. port .. '/')
end

--- The command line that starts the service: the shared flags sit on `arc`,
--- before the subcommand, the port after it.
--- @param port integer|nil port to serve on; nil lets the OS pick one
--- @param switches cargo-arc.Switches|nil the analysis switches to start
--- with; nil for the configuration's
--- @return string[]
function M.argv(port, switches)
  local config = M.config
  local argv = { config.binary, 'arc' }
  if config.manifest_path then
    vim.list_extend(argv, { '--manifest-path', config.manifest_path })
  end
  if #config.features > 0 then
    vim.list_extend(argv, { '--features', table.concat(config.features, ',') })
  end
  local wanted = switches or { externals = config.externals, tests = config.include_tests }
  if wanted.tests then
    table.insert(argv, '--include-tests')
  end
  if wanted.externals then
    table.insert(argv, '--externals')
  end
  table.insert(argv, 'ui')
  if port then
    vim.list_extend(argv, { '--port', tostring(port) })
  end
  return argv
end

--- Feeds `line_handler` complete lines from a stream that arrives in
--- arbitrary chunks.
--- @param line_handler fun(line: string)
--- @return fun(err: string|nil, data: string|nil)
local function line_splitter(line_handler)
  local pending = ''
  return function(_, data)
    if data == nil then
      return
    end
    pending = pending .. data
    while true do
      local newline = pending:find('\n', 1, true)
      if not newline then
        break
      end
      local line = pending:sub(1, newline - 1)
      pending = pending:sub(newline + 1)
      vim.schedule(function()
        line_handler(line)
      end)
    end
  end
end

--- Writes one line to the service's stdin. A service that was asked to
--- stop still exists until its exit callback runs; it gets nothing more.
--- @param line string
local function send(line)
  if not service or service.stopping then
    return
  end
  service.process:write(line .. '\n')
end

--- Tells the service where the cursor is, if the current buffer is a file.
--- The service decides whether the file has a node; nothing is filtered
--- here beyond buffers that are no file at all.
local function send_focus()
  if vim.bo.buftype ~= '' then
    return
  end
  local name = vim.api.nvim_buf_get_name(0)
  if name == '' then
    return
  end
  local line = vim.api.nvim_win_get_cursor(0)[1]
  send('arc focus ' .. line .. ' ' .. vim.fn.fnamemodify(name, ':p'))
end

local follow_group = 'cargo-arc-follow'

--- Reports the cursor's file whenever it can have changed: on entering a
--- buffer and on the editor regaining focus.
local function watch_cursor()
  local group = vim.api.nvim_create_augroup(follow_group, {})
  vim.api.nvim_create_autocmd({ 'BufEnter', 'FocusGained' }, {
    group = group,
    callback = send_focus,
  })
end

--- Starts the service for the current working directory. With `replaces`,
--- the port of the service that just exited, the new one takes that port
--- and the page that shows it stays open; with `switches`, the state that
--- service last reached, it starts with that state.
--- @param replaces integer|nil
--- @param switches cargo-arc.Switches|nil
local function start(replaces, switches)
  local started = { replaces = replaces }
  local ok, process = pcall(vim.system, M.argv(replaces, switches), {
    cwd = vim.fn.getcwd(),
    text = true,
    stdin = true,
    stdout = line_splitter(M.handle_line),
  }, function(result)
    vim.schedule(function()
      service = nil
      pcall(vim.api.nvim_del_augroup_by_name, follow_group)
      if not started.port and not started.stopping then
        vim.notify(
          'cargo-arc ended with exit code ' .. result.code .. ' before announcing a port\n' .. result.stderr,
          vim.log.levels.ERROR
        )
      end
      if started.restart then
        start(started.port, started.switches)
      end
    end)
  end)
  if not ok then
    vim.notify('cargo-arc: cannot start ' .. M.config.binary .. ': ' .. process, vim.log.levels.ERROR)
    return
  end
  started.process = process
  service = started
  watch_cursor()
end

--- Starts the service for the current working directory, or reopens the
--- page when it is already running.
function M.open()
  if service then
    if service.port then
      open_page(service.port)
    else
      vim.notify('cargo-arc is still starting', vim.log.levels.INFO)
    end
    return
  end
  start(nil)
end

--- Ends the service. Its exit callback clears the state.
function M.stop()
  if service then
    service.stopping = true
    service.process:kill('sigterm')
  end
end

--- Ends the service and starts a new one on the same port once it has
--- exited, so a reload of the open page shows the code as it is now.
function M.restart()
  if not service then
    M.open()
    return
  end
  service.restart = true
  M.stop()
end

--- Writes `arc <name> on|off`, the line every switch is spelled as.
--- @param name string
--- @param on boolean
local function send_switch(name, on)
  if not service then
    vim.notify('cargo-arc is not running', vim.log.levels.INFO)
    return
  end
  send('arc ' .. name .. (on and ' on' or ' off'))
end

--- Switches whether the page follows the cursor. The service passes the
--- state through; the page holds it.
--- @param on boolean
function M.follow(on)
  send_switch('follow', on)
end

--- Asks the service to take external crates into the analysis or leave
--- them out; it recomputes and reports the state it reached.
--- @param on boolean
function M.externals(on)
  send_switch('externals', on)
end

--- Asks the service to take test code into the analysis or leave it out.
--- @param on boolean
function M.tests(on)
  send_switch('tests', on)
end

--- The window in the current tabpage that shows `file`, if any.
--- @param file string absolute path
--- @return integer|nil
local function window_showing(file)
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(win)) == file then
      return win
    end
  end
end

--- Puts the cursor on `line` of `file`: in the window that already shows
--- the file, otherwise in the current window after opening it there. A line
--- that was not on screen is centered; one that was keeps the scroll.
--- @param file string absolute path
--- @param line integer
function M.jump(file, line)
  local win = window_showing(file)
  if win then
    vim.api.nvim_set_current_win(win)
  else
    vim.cmd.edit(vim.fn.fnameescape(file))
  end
  -- The file may have grown shorter since the service analyzed it.
  line = math.min(line, vim.api.nvim_buf_line_count(0))
  local was_visible = vim.fn.line('w0') <= line and line <= vim.fn.line('w$')
  vim.api.nvim_win_set_cursor(0, { line, 0 })
  if not was_visible then
    vim.cmd('normal! zz')
  end
end

--- Handles one line of the service's stdout.
--- @param line string
function M.handle_line(line)
  local version, port = line:match('^arc ready (%S+) (%d+)$')
  if port then
    port = tonumber(port)
    if service then
      service.version = version
      service.port = port
    end
    if service and service.replaces then
      vim.notify('cargo-arc ' .. version .. ' restarted on port ' .. port .. '; reload the page', vim.log.levels.INFO)
    else
      open_page(port)
    end
    return
  end
  local lnum, file = line:match('^arc jump (%d+) (.+)$')
  if file then
    M.jump(file, tonumber(lnum))
    return
  end
  local externals, tests = line:match('^arc analysis externals=(%a+) tests=(%a+)$')
  if externals then
    local reached = { externals = externals == 'on', tests = tests == 'on' }
    -- The first report only tells the plugin the state; a later one that
    -- differs is the outcome of a switch and worth a notice.
    if service and service.switches and not vim.deep_equal(service.switches, reached) then
      vim.notify(
        'cargo-arc: external crates ' .. externals .. ', test code ' .. tests,
        vim.log.levels.INFO
      )
    end
    if service then
      service.switches = reached
    end
    return
  end
  local error_text = line:match('^arc analysis%-error (.+)$')
  if error_text then
    vim.notify('cargo-arc: the analysis failed: ' .. error_text, vim.log.levels.ERROR)
  end
end

return M
