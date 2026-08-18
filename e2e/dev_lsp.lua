-- Shared dev-nvim LSP setup for perl-lsp (used by init.lua and init_cpp.lua).
-- One place for the binary resolution, debug wiring, vim.lsp.config, and the
-- LspAttach keymaps/completion/sig-help — so every language gets the same DX.
--
-- Usage (from a `-u` init script):
--   local here = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h")
--   dofile(here .. "/dev_lsp.lua")({
--     filetypes = { "perl" },
--     root_markers = { ".git", "cpanfile" },
--     attach_message = "perl-lsp attached! ...",
--   })

return function(opts)
  opts = opts or {}

  -- Minimal editor settings (completion popup, signs, inlay room)
  vim.opt.number = true
  vim.opt.signcolumn = "yes"
  vim.opt.updatetime = 300
  vim.opt.completeopt = { "menuone", "noselect", "popup" }
  vim.opt.pumheight = 15

  -- Window navigation (muscle memory): Ctrl-h/j/k/l
  vim.keymap.set("n", "<C-h>", "<C-w>h")
  vim.keymap.set("n", "<C-j>", "<C-w>j")
  vim.keymap.set("n", "<C-k>", "<C-w>k")
  vim.keymap.set("n", "<C-l>", "<C-w>l")

  -- aerial.nvim outline, OPTIMISTIC + interactive-only: never loads headless
  -- (e2e stays plugin-free), never errors if the clone is absent/broken, and
  -- lives outside the global nvim config (~/.local/share/nvim-dev-plugins).
  if #vim.api.nvim_list_uis() > 0 then
    local aerial_path = vim.fn.expand("~/.local/share/nvim-dev-plugins/aerial.nvim")
    if vim.fn.isdirectory(aerial_path) == 1 then
      vim.opt.runtimepath:prepend(aerial_path)
      local ok, aerial = pcall(require, "aerial")
      if ok then
        pcall(aerial.setup, {
          backends = { "lsp" },
          layout = { default_direction = "prefer_right", min_width = 32 },
          show_guides = true,
        })
        vim.keymap.set("n", "<leader>a", "<cmd>AerialToggle!<CR>")
        vim.keymap.set("n", "{", "<cmd>AerialPrev<CR>")
        vim.keymap.set("n", "}", "<cmd>AerialNext<CR>")
      end
    end
  end

  -- Indexing progress spinner, interactive-only (never headless — e2e stays
  -- quiet + plugin-free). The server emits window/workDoneProgress for the
  -- workspace index (perl + pack tokens); this shows a top-right spinner so you
  -- KNOW smarts are still warming during the cold-open window and WHEN they
  -- land, instead of waiting blind. Dependency-free: nvim's built-in
  -- `LspProgress` autocmd + a small float. Tracks BOTH tokens; clears when both
  -- End.
  if #vim.api.nvim_list_uis() > 0 then
    local uv = vim.uv or vim.loop
    local frames = { "⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏" }
    local st = { win = nil, buf = nil, timer = nil, fi = 1, active = {}, msg = "" }
    local function close()
      if st.timer then pcall(function() st.timer:stop(); st.timer:close() end); st.timer = nil end
      if st.win and vim.api.nvim_win_is_valid(st.win) then pcall(vim.api.nvim_win_close, st.win, true) end
      st.win, st.buf = nil, nil
    end
    local function render(spin)
      if not (st.win and vim.api.nvim_win_is_valid(st.win)) then
        st.buf = vim.api.nvim_create_buf(false, true)
        st.win = vim.api.nvim_open_win(st.buf, false, {
          relative = "editor", anchor = "NE", row = 1, col = vim.o.columns - 1,
          width = 44, height = 1, style = "minimal", border = "rounded",
          focusable = false, noautocmd = true, zindex = 200,
        })
      end
      local line = (spin and (frames[st.fi] .. " ") or "") .. "perl-lsp: " .. st.msg
      pcall(vim.api.nvim_buf_set_lines, st.buf, 0, -1, false, { line:sub(1, 44) })
    end
    vim.api.nvim_create_autocmd("LspProgress", {
      callback = function(ev)
        local p = ev.data and ev.data.params
        local v = p and p.value
        if not v then return end
        local tok = tostring(p.token or "")
        if not tok:match("workspace%-index") then return end
        if v.kind == "begin" or v.kind == "report" then
          st.active[tok] = true
          st.msg = (v.title or "Indexing")
            .. (v.percentage and (" " .. v.percentage .. "%") or "")
            .. (v.message and (" — " .. v.message) or "")
          if not st.timer then
            st.timer = uv.new_timer()
            st.timer:start(0, 90, vim.schedule_wrap(function()
              st.fi = st.fi % #frames + 1
              if next(st.active) then render(true) else close() end
            end))
          end
        elseif v.kind == "end" then
          st.active[tok] = nil
          if not next(st.active) then
            st.msg = "✓ " .. (v.message or "indexed") .. " — full smarts ready"
            render(false)
            vim.defer_fn(close, 2500)
          end
        end
      end,
    })
  end

  -- Built binary. Override with PERL_LSP_BIN for comparison runs.
  local lsp_bin = vim.env.PERL_LSP_BIN
    and vim.fn.fnamemodify(vim.env.PERL_LSP_BIN, ":p")
    or vim.fn.fnamemodify("target/release/perl-lsp", ":p")

  -- Debug mode: PERL_LSP_DEBUG=1 → RUST_LOG to /tmp/perl-lsp.log
  local debug_mode = vim.env.PERL_LSP_DEBUG == "1"
  local log_file = "/tmp/perl-lsp.log"
  local cmd
  if debug_mode then
    cmd = {
      "sh", "-c",
      "RUST_LOG=perl_lsp=debug exec " .. vim.fn.shellescape(lsp_bin) .. " 2>>" .. log_file,
    }
  else
    cmd = { lsp_bin }
  end

  -- initializationOptions. PERL_LSP_COLD_WAIT_MILLISECONDS overrides the cold-open
  -- bounded-wait cap (0 opts the wait out) — the e2e cold-window repro toggles
  -- the fix on/off from the same binary through it.
  local init_options = nil
  if vim.env.PERL_LSP_COLD_WAIT_MILLISECONDS then
    init_options = { coldWaitMs = tonumber(vim.env.PERL_LSP_COLD_WAIT_MILLISECONDS) }
  end

  vim.lsp.config["perl-lsp"] = {
    cmd = cmd,
    filetypes = opts.filetypes or { "perl" },
    root_markers = opts.root_markers or { ".git" },
    init_options = init_options,
  }
  vim.lsp.enable("perl-lsp")

  -- Keybindings + DX, set up on LspAttach (shared across languages)
  vim.api.nvim_create_autocmd("LspAttach", {
    callback = function(args)
      local buf = args.buf
      local client_id = args.data.client_id
      local kopts = { buffer = buf }

      -- Built-in LSP completion (nvim 0.11+), autotrigger on server triggers
      vim.lsp.completion.enable(true, client_id, buf, { autotrigger = true })
      vim.lsp.inlay_hint.enable(true, { bufnr = buf })

      -- Navigation
      vim.keymap.set("n", "gd", vim.lsp.buf.definition, kopts)
      vim.keymap.set("n", "gi", vim.lsp.buf.implementation, kopts)
      vim.keymap.set("n", "gr", vim.lsp.buf.references, kopts)
      vim.keymap.set("n", "K", vim.lsp.buf.hover, kopts)

      -- Rename / outline
      vim.keymap.set("n", "<leader>rn", vim.lsp.buf.rename, kopts)
      vim.keymap.set("n", "<leader>o", vim.lsp.buf.document_symbol, kopts)

      -- Document highlight: symbol under cursor
      vim.api.nvim_create_autocmd({ "CursorHold", "CursorHoldI" }, {
        buffer = buf,
        callback = vim.lsp.buf.document_highlight,
      })
      vim.api.nvim_create_autocmd("CursorMoved", {
        buffer = buf,
        callback = vim.lsp.buf.clear_references,
      })

      -- Smart expand/shrink selection (selectionRange): + parent, - child
      local sel_stack = {}
      local function clamp(lnum, col)
        local last_line = vim.api.nvim_buf_line_count(buf)
        lnum = math.max(1, math.min(lnum, last_line))
        local line_text = vim.api.nvim_buf_get_lines(buf, lnum - 1, lnum, false)[1] or ""
        col = math.max(0, math.min(col, math.max(0, #line_text - 1)))
        return lnum, col
      end
      local function flatten_sr(node)
        local ranges = {}
        while node do
          table.insert(ranges, node.range)
          node = node.parent
        end
        return ranges
      end
      local function set_visual(r)
        local sl, sc = clamp(r.start.line + 1, r.start.character)
        local el, ec = clamp(r["end"].line + 1, math.max(0, r["end"].character - 1))
        vim.cmd("normal! \\<Esc>")
        vim.api.nvim_win_set_cursor(0, { sl, sc })
        vim.cmd("normal! v")
        vim.api.nvim_win_set_cursor(0, { el, ec })
      end
      vim.keymap.set({ "n", "v" }, "+", function()
        local sr = vim.lsp.buf_request_sync(buf, "textDocument/selectionRange", {
          textDocument = vim.lsp.util.make_text_document_params(buf),
          positions = { vim.lsp.util.make_position_params(0, "utf-16").position },
        }, 1000)
        if not sr then return end
        for _, res in pairs(sr) do
          if res.result and res.result[1] then
            local ranges = flatten_sr(res.result[1])
            local idx = #sel_stack + 1
            if idx <= #ranges then
              sel_stack[idx] = ranges[idx]
              set_visual(ranges[idx])
            end
            return
          end
        end
      end, kopts)
      vim.keymap.set("v", "-", function()
        if #sel_stack > 1 then
          table.remove(sel_stack)
          set_visual(sel_stack[#sel_stack])
        elseif #sel_stack == 1 then
          sel_stack = {}
          vim.cmd("normal! \\<Esc>")
        end
      end, kopts)
      vim.api.nvim_create_autocmd("ModeChanged", {
        pattern = "v:n",
        callback = function() sel_stack = {} end,
      })

      -- Signature help: trigger on ( and , ; re-trigger inside parens
      vim.keymap.set("i", "<C-s>", vim.lsp.buf.signature_help, kopts)
      vim.api.nvim_create_autocmd("TextChangedI", {
        buffer = buf,
        callback = function()
          local col = vim.fn.col(".") - 1
          if col <= 0 then return end
          local line = vim.api.nvim_get_current_line()
          local before = line:sub(1, col)
          local char = before:sub(-1)
          if char == "(" or char == "," then
            vim.schedule(function()
              if vim.fn.mode() == "i" then vim.lsp.buf.signature_help() end
            end)
            return
          end
          local opens = select(2, before:gsub("%(", ""))
          local closes = select(2, before:gsub("%)", ""))
          if opens > closes then
            vim.schedule(function()
              if vim.fn.mode() == "i" then vim.lsp.buf.signature_help() end
            end)
          end
        end,
      })

      -- Format + diagnostics nav
      vim.keymap.set("n", "<leader>f", vim.lsp.buf.format, kopts)
      vim.keymap.set("n", "[d", vim.diagnostic.goto_prev, kopts)
      vim.keymap.set("n", "]d", vim.diagnostic.goto_next, kopts)

      -- Manual completion (C-Space) + bareword autotrigger
      vim.keymap.set("i", "<C-Space>", function() vim.lsp.completion.get() end, kopts)
      vim.api.nvim_create_autocmd("InsertCharPre", {
        buffer = buf,
        callback = function()
          if vim.fn.pumvisible() == 1 then return end
          local char = vim.v.char
          if not char:match("[%w_]") then return end
          local col = vim.fn.col(".") - 1
          if col <= 0 then return end
          local line = vim.api.nvim_get_current_line()
          local word = line:sub(1, col):match("[%a_][%w_:]*$")
          if not word then return end
          vim.schedule(function()
            if vim.fn.mode() == "i" and vim.fn.pumvisible() == 0 then
              vim.lsp.completion.get()
            end
          end)
        end,
      })

      print(opts.attach_message or "perl-lsp attached! gd=def gi=impl gr=refs K=hover <leader>rn=rename <leader>o=symbols <leader>f=format")
    end,
  })
end
