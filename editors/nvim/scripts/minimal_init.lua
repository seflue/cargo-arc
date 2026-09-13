-- Runtime for the test runner and for every child Neovim it starts: the
-- plugin itself and mini.nvim from deps/, nothing from the user's config.
vim.cmd([[let &runtimepath .= ',' .. getcwd()]])
vim.cmd([[let &runtimepath .= ',' .. getcwd() .. '/deps/mini.nvim']])
require('mini.test').setup()
