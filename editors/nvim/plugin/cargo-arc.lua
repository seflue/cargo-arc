if vim.g.loaded_cargo_arc then
  return
end
vim.g.loaded_cargo_arc = true

local subcommands = { 'open', 'restart', 'stop' }

vim.api.nvim_create_user_command('Arc', function(command)
  local subcommand = command.fargs[1]
  if not vim.list_contains(subcommands, subcommand) then
    vim.notify(
      'cargo-arc: unknown subcommand "' .. tostring(subcommand) .. '"; use one of '
        .. table.concat(subcommands, ', '),
      vim.log.levels.ERROR
    )
    return
  end
  require('cargo-arc')[subcommand]()
end, {
  nargs = 1,
  complete = function(prefix)
    return vim.tbl_filter(function(name)
      return vim.startswith(name, prefix)
    end, subcommands)
  end,
  desc = 'Start, restart or stop the cargo-arc diagram service',
})

-- The service is a child of this Neovim and must not outlive it.
vim.api.nvim_create_autocmd('VimLeavePre', {
  group = vim.api.nvim_create_augroup('cargo-arc', {}),
  callback = function()
    require('cargo-arc').stop()
  end,
})
