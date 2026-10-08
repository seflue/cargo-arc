-- lazy.nvim runs this file after installing or updating the plugin. It
-- installs the crates.io release of this checkout's version, because that
-- does not require Bun.

local checkout = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':p:h')
-- require would make lazy.nvim load the plugin and run its setup with the
-- user's options here, during the build.
local plugin = dofile(vim.fs.joinpath(checkout, 'lua', 'cargo-arc', 'init.lua'))
local root = plugin.install_root()
local version = plugin.package_version(vim.fs.joinpath(checkout, 'Cargo.toml'))
if not version then
  error('cargo-arc: no package version in ' .. vim.fs.joinpath(checkout, 'Cargo.toml'))
end

if vim.fn.executable('cargo') ~= 1 then
  error('cargo-arc: installing cargo-arc needs cargo on PATH')
end
vim.fn.mkdir(root, 'p')

local result = nil
-- cargo reports its progress on stderr.
local lines = {}
local pending = ''
vim.system({ 'cargo', 'install', '--locked', '--version', '=' .. version, '--root', root, 'cargo-arc' }, {
  -- Inside the checkout, rust-toolchain.toml would select its toolchain.
  cwd = root,
  text = true,
  stderr = function(_, data)
    if data == nil then
      if pending ~= '' then
        table.insert(lines, pending)
      end
      return
    end
    pending = pending .. data
    for line in pending:gmatch('([^\n]*)\n') do
      table.insert(lines, line)
    end
    pending = pending:match('[^\n]*$')
  end,
}, function(exit)
  result = exit
end)

local shown = 0
while not result do
  if #lines > shown then
    shown = #lines
    coroutine.yield({ msg = vim.trim(lines[shown]), level = vim.log.levels.TRACE })
  else
    coroutine.yield()
  end
end

if result.code ~= 0 then
  -- Show only the tail, because cargo prints its error after all progress.
  local error_lines = 30
  local tail = vim.list_slice(lines, math.max(1, #lines - error_lines + 1))
  error('cargo-arc: cargo install failed with exit code ' .. result.code .. ':\n' .. table.concat(tail, '\n'))
end
coroutine.yield({ msg = 'cargo-arc installed in ' .. root, level = vim.log.levels.INFO })
