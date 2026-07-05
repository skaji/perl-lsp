-- Cold-open degraded-window HEAL probe (hitlist-4 Family B).
--
-- Opens a C file in a BIG tree (perl5) whose pack workspace index takes many
-- seconds to attach. A references query issued inside that window sees a
-- DEGRADED answer (only the local def, cross-file uses absent). The probe then
-- POLLS the same query, without re-opening the file, and reports whether — and
-- when — the answer heals to the full cross-file set.
--
-- Pre-fix: the window's degraded doc-analysis is never re-derived after the
-- index lands (nothing re-publishes / re-analyzes the open doc), so the heal is
-- driven only by the live references query racing the index; the doc's baked
-- state stays cold. Post-fix: `ensure_workspace_indexed` completion re-analyzes
-- every open pack doc + re-publishes, so the heal is a server-driven event.
--
-- Env:
--   HEAL_FILE   absolute path to the C file to open (default perl5/op.c)
--   HEAL_ROW    0-indexed row of the def name (default 899 = op.c:900 op_free)
--   HEAL_COL    0-indexed col of the def name (default 0)
--   HEAL_MAXMS  poll ceiling in ms (default 60000)
vim.opt.rtp:prepend(".")
local lsp = require("test.lsp")
lsp.timeout_ms = 20000

local file   = vim.env.HEAL_FILE or "/home/veesh/personal/perl5/op.c"
local row    = tonumber(vim.env.HEAL_ROW) or 899
local col    = tonumber(vim.env.HEAL_COL) or 0
local maxms  = tonumber(vim.env.HEAL_MAXMS) or 60000

local buf = lsp.open_and_attach(file)

local function ref_count()
  local lines = lsp.reference_lines(buf, row, col)
  return lines and #lines or 0
end

-- t=0: the in-window answer (open_and_attach already waited 500ms, but on a
-- big tree the pack index is nowhere near done).
local in_window = ref_count()
io.write(string.format("in_window_refs=%d\n", in_window))

-- Poll WITHOUT touching the buffer. Record the first time the count exceeds
-- the in-window degraded answer (the heal), and the settled maximum.
local start = vim.loop.now()
local healed_at = nil
local settled = in_window
local step = 500
local waited = 0
while waited < maxms do
  vim.wait(step)
  waited = vim.loop.now() - start
  local c = ref_count()
  if c > settled then
    settled = c
    if not healed_at then healed_at = waited end
  end
  -- Stop early once it clearly healed and held for one extra poll.
  if healed_at and (waited - healed_at) >= step and settled > in_window then
    break
  end
end

io.write(string.format("settled_refs=%d\n", settled))
io.write(string.format("healed_at_ms=%s\n", healed_at and tostring(healed_at) or "NEVER"))
io.write(string.format("window_ms=%s\n", healed_at and tostring(healed_at) or ">" .. tostring(maxms)))

if settled > in_window and healed_at then
  io.write("RESULT: HEALED\n")
  vim.cmd("qa!")
else
  io.write("RESULT: NO-HEAL\n")
  vim.cmd("cquit! 1")
end
