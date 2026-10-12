local core = require("beta.core")
local api = require("vim.api")

local M = {}

function M.prepare()
  return core.value(api)
end

return M
