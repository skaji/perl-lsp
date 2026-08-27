---
name: dev-setup
description: Install the two things a fresh box lacks before the full test suite can run — the gold-corpus CPAN substrate (gold-corpus/local/) and nvim 0.11+ for e2e. Use in any CLOUD or container session, on a new machine, or whenever gold reports mass lang-skips or e2e fails with a nvim/get_clients error. Neither is in the repo; both must be installed per box.
---

# dev-setup: what a fresh box is missing

`cargo build` and `cargo test` work anywhere. The other two suites do not — they
depend on artifacts that live outside the repo and are absent on every new
machine:

| Suite | Needs | Symptom when missing |
|---|---|---|
| `gold-corpus/run.pl` | `gold-corpus/local/` (CPAN substrate) | mass **lang-skips**, not failures — the summary lies healthy |
| `./e2e/run.sh` | `nvim` **0.11+** | harness aborts on `vim.lsp.get_clients` |

**This matters most in cloud sessions and containers.** Every one starts from a
clean image: neither artifact is checked in, neither survives the previous
session, and both fail QUIETLY — a skipped gold row and an aborted e2e run both
look like "not my change." A cloud session that skips this setup can only ever
run `cargo test`, which is a third of the net. Do the setup; do not conclude
"CI will cover it."

## Gold substrate (`gold-corpus/local/`)

Needs `cpm`. In a sandbox where the proxy 403s the metacpan metadata hosts
(`fastapi.metacpan.org`, `cpanmetadb.plackperl.org`) but allows `www.cpan.org`
tarball/`02packages` paths, point cpm at the cpan.org mirror — and install
`Carton::Snapshot` first, since the `snapshot` resolver requires it:

```
curl -sSL https://raw.githubusercontent.com/skaji/cpm/main/cpm -o /tmp/cpm && chmod +x /tmp/cpm
/tmp/cpm install -g --mirror https://www.cpan.org/ --resolver 02packages Carton::Snapshot
cd gold-corpus && /tmp/cpm install -L local --mirror https://www.cpan.org/ --resolver snapshot
```

Outside a restricted sandbox, plain `cpm install -L local --resolver snapshot`
or `carton install --deployment` from `gold-corpus/` works.

Verify: run the harness and check `lang-skip 0` in the summary. A plain release
build also lang-skips 253 rows on its own — build `--features cpp`.

## nvim for e2e

`e2e/run.sh` needs `nvim` 0.10+ (the harness calls `vim.lsp.get_clients`).
Ubuntu ships 0.9.5, which fails. The release tarball is a 10 MB download and
needs no sudo and no package manager:

```
curl -sSL -o /tmp/nvim.tar.gz \
  https://github.com/neovim/neovim/releases/download/v0.11.0/nvim-linux-x86_64.tar.gz
tar xzf /tmp/nvim.tar.gz -C /tmp
export PATH=/tmp/nvim-linux-x86_64/bin:$PATH   # nvim --version -> NVIM v0.11.0
```

v0.11.0 is the version CI pins (`rhysd/action-setup-vim`), so a local pass means
the same thing a CI pass does.
