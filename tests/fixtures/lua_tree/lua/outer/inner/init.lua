local M = {}

function M.load()
  return require [[outer.leaf]]
end

M.deep = require("outer.nested.deep")

return M
