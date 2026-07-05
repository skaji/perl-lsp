-- Cold-open BOUNDED-WAIT probe (hitlist-4 Family B, the ledgered pull-verb
-- residual).
--
-- Fires ONE references query inside the first-open window — the single degraded
-- answer a user fires and never re-triggers — and reports the count + the
-- handler's wall-clock latency. It does NOT poll: the whole point is that ONE
-- in-window query resolves warm because the HANDLER waited for the imminent
-- index, not because a later re-request raced it.
--
--   PERL_LSP_COLD_WAIT_MS unset / large  → handler waits → full cross-file set.
--   PERL_LSP_COLD_WAIT_MS=0              → wait opts out → degraded (def only).
--
-- After the in-window query, it waits for the index to fully settle then fires
-- the SAME query again and measures its latency — the WARM case must pay ~zero
-- added wait (index already done → the bounded wait returns before awaiting).
--
-- Env:
--   HEAL_FILE   absolute path to the C file to open (default perl5/op.c)
--   HEAL_ROW    0-indexed row of the def name (default 899 = op.c:900 op_free)
--   HEAL_COL    0-indexed col of the def name (default 0)
--   WARM_WAITMS how long to let the index settle before the warm query (25000)
vim.opt.rtp:prepend(".")
local lsp = require("test.lsp")
lsp.timeout_ms = 30000

local file      = vim.env.HEAL_FILE or "/home/veesh/personal/perl5/op.c"
local row       = tonumber(vim.env.HEAL_ROW) or 899
local col       = tonumber(vim.env.HEAL_COL) or 0
local warm_wait = tonumber(vim.env.WARM_WAITMS) or 25000

local buf = lsp.open_and_attach(file)

local function timed_ref_count()
  local t0 = vim.loop.now()
  local lines = lsp.reference_lines(buf, row, col)
  local dt = vim.loop.now() - t0
  return (lines and #lines or 0), dt
end

-- The single in-window pull query. `open_and_attach` waited 500ms; on the big
-- perl5 tree the pack index is nowhere near done, so this handler either waits
-- for it (fix on) or answers degraded (fix off).
local in_window, in_window_ms = timed_ref_count()
io.write(string.format("in_window_refs=%d\n", in_window))
io.write(string.format("in_window_ms=%d\n", in_window_ms))

-- Let the index fully settle, then re-fire: the WARM case. The bounded wait
-- must see `done` set and return before awaiting, so this pays ~zero added
-- latency (dominated only by the resolve itself).
vim.wait(warm_wait)
local warm, warm_ms = timed_ref_count()
io.write(string.format("warm_refs=%d\n", warm))
io.write(string.format("warm_ms=%d\n", warm_ms))

-- Green iff the single in-window query already saw the FULL cross-file set
-- (the handler waited for the index) — measured against the warm baseline, so
-- the local-only degraded count (whatever it is) doesn't have to be guessed.
if warm > 0 and in_window >= warm then
  io.write("RESULT: WAIT-HEALED\n")
  vim.cmd("qa!")
else
  io.write("RESULT: DEGRADED (no wait)\n")
  vim.cmd("cquit! 1")
end
