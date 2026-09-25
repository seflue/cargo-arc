if vim.g.loaded_cargo_arc then
  return
end
vim.g.loaded_cargo_arc = true

local subcommands = { 'open', 'restart', 'stop', 'recompute', 'follow', 'externals', 'tests', 'on-save' }
--- The subcommands that take `on` or `off`.
local switches = { 'follow', 'externals', 'tests', 'on-save' }
local states = { 'on', 'off' }

local function report_unknown(what, value, choices)
  vim.notify(
    'cargo-arc: unknown ' .. what .. ' "' .. tostring(value) .. '"; use one of ' .. table.concat(choices, ', '),
    vim.log.levels.ERROR
  )
end

vim.api.nvim_create_user_command('Arc', function(command)
  local subcommand = command.fargs[1]
  if not vim.list_contains(subcommands, subcommand) then
    report_unknown('subcommand', subcommand, subcommands)
    return
  end
  -- `on-save` is the Lua function `on_save`.
  local handler = require('cargo-arc')[subcommand:gsub('-', '_')]
  if vim.list_contains(switches, subcommand) then
    local state = command.fargs[2]
    if not vim.list_contains(states, state) then
      report_unknown(subcommand .. ' state', state, states)
      return
    end
    handler(state == 'on')
    return
  end
  if #command.fargs > 1 then
    vim.notify('cargo-arc: ' .. subcommand .. ' takes no argument', vim.log.levels.ERROR)
    return
  end
  handler()
end, {
  nargs = '+',
  complete = function(prefix, line)
    local words = vim.split(line, '%s+', { trimempty = true })
    -- Past the subcommand, only the switches take an argument.
    local at_subcommand = #words == 1 or (#words == 2 and prefix ~= '')
    local choices = at_subcommand and subcommands or (vim.list_contains(switches, words[2]) and states or {})
    return vim.tbl_filter(function(name)
      return vim.startswith(name, prefix)
    end, choices)
  end,
  desc = 'Start, restart or stop the cargo-arc diagram service, recompute it, or switch following, external crates, test code or recomputing on save',
})

-- The service is a child of this Neovim and must not outlive it.
vim.api.nvim_create_autocmd('VimLeavePre', {
  group = vim.api.nvim_create_augroup('cargo-arc', {}),
  callback = function()
    require('cargo-arc').stop()
  end,
})
