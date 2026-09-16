if vim.g.loaded_cargo_arc then
  return
end
vim.g.loaded_cargo_arc = true

local subcommands = { 'open', 'restart', 'stop', 'follow' }
local follow_states = { 'on', 'off' }

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
  if subcommand == 'follow' then
    local state = command.fargs[2]
    if not vim.list_contains(follow_states, state) then
      report_unknown('follow state', state, follow_states)
      return
    end
    require('cargo-arc').follow(state == 'on')
    return
  end
  if #command.fargs > 1 then
    vim.notify('cargo-arc: ' .. subcommand .. ' takes no argument', vim.log.levels.ERROR)
    return
  end
  require('cargo-arc')[subcommand]()
end, {
  nargs = '+',
  complete = function(prefix, line)
    local words = vim.split(line, '%s+', { trimempty = true })
    -- Past the subcommand, only `follow` takes an argument.
    local at_subcommand = #words == 1 or (#words == 2 and prefix ~= '')
    local choices = at_subcommand and subcommands or (words[2] == 'follow' and follow_states or {})
    return vim.tbl_filter(function(name)
      return vim.startswith(name, prefix)
    end, choices)
  end,
  desc = 'Start, restart or stop the cargo-arc diagram service, or switch following the editor',
})

-- The service is a child of this Neovim and must not outlive it.
vim.api.nvim_create_autocmd('VimLeavePre', {
  group = vim.api.nvim_create_augroup('cargo-arc', {}),
  callback = function()
    require('cargo-arc').stop()
  end,
})
