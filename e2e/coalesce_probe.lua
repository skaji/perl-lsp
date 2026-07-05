-- Coalesce probe: open a Perl file whose `use`s trigger a burst of module
-- resolutions, wait for the resolver storm to settle, then quit. The
-- PERL_LSP_DEBUG log's `diag-refresh fired` vs `diag-refresh executing` line
-- counts are the coalesce before/after.
vim.opt.rtp:prepend(".")
local lsp = require("test.lsp")
local file = vim.env.STORM_FILE
local buf = lsp.open_and_attach(file)
-- Nudge a query so diagnostics/refresh paths run, then let the storm land.
lsp.hover_text(buf, 0, 0)
vim.wait(tonumber(vim.env.STORM_WAITMS) or 12000)
io.write("coalesce probe done\n")
vim.cmd("qa!")
