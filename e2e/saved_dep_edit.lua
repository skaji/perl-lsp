-- E2E: a SAVED edit to a dependency becomes visible to its consumer
-- mid-session, no restart — through a cross-file RETURN TYPE, which is the
-- path the conclusion layer answers on.
--
-- The consumer's receiver comes from `Provider::build`'s return type, so the
-- verb under test resolves through the provider's baked conclusion map rather
-- than through anything in the consumer's own file. A bake that outlived the
-- blob it came from answers this with full confidence from the PREVIOUS
-- version of the provider, and every other cross-file test still passes.
-- Usage: PERL_LSP_BIN=target/release/perl-lsp \
--          nvim --headless --clean -u e2e/init.lua -l e2e/saved_dep_edit.lua
vim.opt.rtp:prepend(".")
local t   = require("test.runner")
local lsp = require("test.lsp")

-- Fresh workspace in a temp dir (never dirties the repo). A `.git` marker
-- makes it the LSP root so the workspace index covers it.
local ws = vim.fn.tempname()
vim.fn.mkdir(ws .. "/Widget", "p")
vim.fn.mkdir(ws .. "/.git", "p")

local provider = ws .. "/Provider.pm"
local consumer = ws .. "/Consumer.pm"

vim.fn.writefile({
  "package Widget::One;",
  "sub go { return 1 }",
  "1;",
}, ws .. "/Widget/One.pm")
vim.fn.writefile({
  "package Widget::Two;",
  "sub go { return 2 }",
  "1;",
}, ws .. "/Widget/Two.pm")

local provider_v1 = {
  "package Provider;",
  "sub build { return bless {}, 'Widget::One' }",
  "1;",
}
local provider_v2 = {
  "package Provider;",
  "sub build { return bless {}, 'Widget::Two' }",
  "1;",
}
vim.fn.writefile(provider_v1, provider)
vim.fn.writefile({
  "package Consumer;",
  "use Provider;",
  "sub run {",
  "    my $w = Provider->build;",
  "    return $w->go;",
  "}",
  "1;",
}, consumer)

local buf = lsp.open_and_attach(consumer)

-- `$w->go` on row 4; the cursor sits inside `go`.
local function go_def() return lsp.def_location(buf, 4, 15) end

local ready = false
for _ = 1, 60 do
  local loc = go_def()
  if loc and loc.uri:find("One%.pm$") then
    ready = true
    break
  end
  vim.wait(500)
end

t.test("baseline: the consumer resolves through the provider's return type", function()
  local N = "baseline cross-file return type"
  if t.ok(N, ready, "$w->go never resolved into Widget/One.pm") then t.pass(N) end
end)

t.test("a saved dependency edit reaches the consumer without restart", function()
  local N = "saved dep edit visible"
  if not t.ok(N, ready, "baseline never warmed; nothing to invalidate") then return end

  vim.cmd("edit " .. vim.fn.fnameescape(provider))
  local pbuf = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(pbuf, 0, -1, false, provider_v2)
  vim.cmd("write")

  local got
  for _ = 1, 60 do
    got = go_def()
    if got and got.uri:find("Two%.pm$") then break end
    vim.wait(500)
  end
  if not t.ok(N, got, "$w->go became unresolvable after the provider save") then return end
  local ok = t.ok(N, got.uri:find("Two%.pm$") ~= nil,
    "still answering from the previous version of the provider: " .. tostring(got.uri))
  if ok then t.pass(N) end
end)

-- With the provider's buffer CLOSED, the open-document tier no longer answers
-- and the query reaches the cached copy — and its baked conclusion map, which
-- the cross-file primary consults BEFORE it decodes anything. A map that
-- outlived the blob it came from answers here, with full confidence, from the
-- version before the save; the previous assertion cannot see that, because an
-- open document outranks the cache.
t.test("the answer survives closing the dependency's buffer", function()
  local N = "closed dep still fresh"
  vim.cmd("edit " .. vim.fn.fnameescape(provider))
  vim.cmd("bdelete!")
  vim.cmd("buffer " .. buf)
  local got
  for _ = 1, 40 do
    got = go_def()
    if got and got.uri:find("Two%.pm$") then break end
    vim.wait(250)
  end
  if not t.ok(N, got, "$w->go unresolvable once the provider buffer closed") then return end
  if t.ok(N, got.uri:find("Two%.pm$") ~= nil,
    "a cached derivation outlived the save: " .. tostring(got.uri)) then t.pass(N) end
end)

t.finish()
