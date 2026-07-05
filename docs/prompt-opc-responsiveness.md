# op.c cold-open responsiveness

## Symptom

Opening `/home/veesh/personal/perl5/op.c` (16 177 lines, macro-heavy C) in the
perl5 tree freezes the editor for ~2 s on first open — the client fires its
on-open verbs (documentSymbol, semanticTokens, foldingRange, inlayHint) and the
first one doesn't return until the freeze clears, so a client with a short
timeout gives up.

## Measured per-request time-to-first-response (cold cache, headless nvim)

First request fired immediately after attach, cache cleared:

| request                     | cold first response | warm |
|-----------------------------|--------------------:|-----:|
| textDocument/documentSymbol | **1666–2150 ms**    | ~7 ms |
| textDocument/foldingRange   | (behind the block)  | <2 ms |
| textDocument/semanticTokens/full | (behind the block) | ~26 ms |
| textDocument/inlayHint      | (behind the block)  | <1 ms |
| textDocument/hover          | ~405 ms (coldWait)  | — |
| textDocument/definition     | ~405 ms (coldWait)  | — |

Firing documentSymbol in a tight loop after attach: the **first** call blocks
1666 ms and returns the full 270-symbol outline; **every subsequent** call is
~1.5 ms. So it is a single head-of-line block, not sustained starvation — the
pack workspace index (spawn_blocking, whole perl5 tree) does not stall the loop.

## Root cause

`did_open` calls `self.files.open(uri, text)` **synchronously** on the async
handler. `FileStore::open` runs the whole pack pipeline (`Document::new_routed`
→ `analyze_with_path`) on the message-loop task before returning. tower-lsp
0.20 does not dispatch the next message until that handler yields, so the 16k
-line build head-of-line-blocks every request the client fires on open.

The build is already run **cached-only** (`set_gather_cached_only(true)`), which
skips the cross-file macro *gather*. But cached-only is **not instant** for a
large file — the intrinsic per-file cost dominates. `--lang-analyze` phase
breakdown for op.c (gather warm, i.e. the cached-only did_open path):

```
cpp.transform      ~915 ms   <- macro-expand + reparse of op.c itself
cpp.member_blocks  ~107 ms
cpp.macro_expand   ~140 ms + ~110 ms
cpp.access_regions  ~86 ms
------------------------------------
per-file build     ~1.3 s   (independent of the cross-file gather)
```

(The cross-file `cpp.gather` is a further ~1.7 s, correctly deferred to the
background `spawn_pack_gather_refresh`.)

So the false premise was "did_open is cached-only ⇒ instant." Cached-only only
removes the *cross-file* gather; the ~1.3 s local macro transform + extraction
still runs synchronously on the message loop.

## Fix (Phase 2)

Move the `FileStore::open` build **off the message loop** (`spawn_blocking`,
awaited inside `did_open`) so the loop stays responsive during the cold build,
and give the on-open read verbs a bounded wait (reusing `coldWaitMs`, the same
machinery as `await_index_ready`) for the in-flight initial build — small/medium
files still return their full answer on the first pull (build < cap); a
pathological file like op.c degrades after the cap and heals when the build
lands. Guard discipline: the wait snapshots the `Notify` Arc and drops all
store/DashMap guards before awaiting.
