#!/usr/bin/env python3
"""sweep.py -- differential answer sweep for perl-lsp.

Three independent steps, so a long corpus run can be resumed or re-diffed
without re-running the expensive half:

  positions  corpus  -> positions.jsonl      (binary-independent)
  run        binary  -> answers-<side>.jsonl (one warm server, thousands of queries)
  diff       two answer files -> a grouped divergence report

THE SERVER PATH, DELIBERATELY. The CLI and the server answer differently for
the same query -- 284,617 vs 193,725 bytes on Koha `references` -- because
they reach different readiness states, and neither is wrong. Mixing them
produces a divergence list that is mostly harness. This drives real LSP over
stdio, like an editor, and the report says so.

Two traps this handles, both of which silently corrupt a comparison:

  * CACHE COLLISION. Both sides key `~/.cache/perl-lsp` off the workspace
    path, and their `EXTRACT_VERSION` / plugin fingerprints differ, so
    sharing a cache dir makes each side hard-clear the other's on every
    startup. Each side gets its own XDG_CACHE_HOME.
  * TIMEOUTS AS EMPTY. A request that times out is recorded as `timeout`,
    never as an empty answer. Folding the two together makes a slower side
    look like it lost answers -- the single most misleading thing a
    differential sweep can report.
"""
import argparse, hashlib, json, os, pathlib, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import positions as P
import normalize as N
# The protocol client is the benchmark driver's, imported rather than copied:
# its fidelity (answering server->client requests, omitting null `params`)
# was bought with real debugging, and a second copy would drift from it.
from lsp_bench import Lsp

VERBS = {
    "definition":     ("textDocument/definition",     "on"),
    "typeDefinition": ("textDocument/typeDefinition", "on"),
    "hover":          ("textDocument/hover",          "on"),
    "references":     ("textDocument/references",     "on"),
    # Completion runs AFTER the token -- the "I just typed this name" slot,
    # which exercises prefix filtering. At `->|` the token start and the
    # member slot coincide, so method-call positions cover both shapes.
    "completion":     ("textDocument/completion",     "after"),
}
# Per-verb sample rate. `references` fans out across the whole workspace and
# is the one verb that can cost seconds per position, so it is sampled rather
# than exhaustive -- the coordinator's call, and the report states the rate.
VERB_RATE = {"references": 0.10}


def cmd_positions(args):
    root = os.path.abspath(args.root)
    files = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in (".git", "blib", "t", ".build")]
        # `auto/share` is installed DATA, not source — DateTime-Locale alone
        # ships hundreds of generated files that are one enormous hash
        # literal. They sample as thousands of hash-key positions that all
        # produce the identical workspace-global completion list, which is
        # one fact repeated, not coverage.
        if os.sep + os.path.join("auto", "share") in os.path.join(dirpath, "x"):
            dirnames[:] = []
            continue
        for fn in filenames:
            if fn.endswith((".pm", ".pl", ".t")):
                files.append(os.path.relpath(os.path.join(dirpath, fn), root))
    files.sort()
    if args.max_files and len(files) > args.max_files:
        files.sort(key=lambda f: P.stable_frac(args.seed, "file", f))
        files = sorted(files[:args.max_files])
    n = 0
    with open(args.out, "w") as fh:
        for rel in files:
            try:
                text = open(os.path.join(root, rel), encoding="utf8", errors="replace").read()
            except OSError:
                continue
            for t in P.select(rel, text, args.per_file, args.seed):
                fh.write(json.dumps(t, sort_keys=True) + "\n")
                n += 1
    print(f"positions: {n} over {len(files)} files (seed={args.seed}, "
          f"per_file={args.per_file})")


def _sha_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()[:16]


def cmd_run(args):
    _t0_wall = time.time()
    root = os.path.abspath(args.root)
    root_uri = pathlib.Path(root).as_uri()
    pos = [json.loads(l) for l in open(args.positions)]

    cache = os.path.abspath(args.cache_dir)
    os.makedirs(cache, exist_ok=True)

    def spawn():
        env = dict(os.environ)
        env["XDG_CACHE_HOME"] = cache
        # A residency panic would abort the sweep mid-corpus; here it is a
        # per-position datum instead, so the run completes and the report can
        # say which positions crashed which side.
        env.setdefault("PERL_LSP_STRICT_RESIDENCY", "0")
        keep = os.environ.copy()
        os.environ.update(env)
        try:
            return Lsp(os.path.abspath(args.bin), root, args.out + ".stderr")
        finally:
            os.environ.clear(); os.environ.update(keep)

    lsp = spawn()

    t_spawn = time.monotonic()
    init, _ = lsp.request("initialize", {
        "processId": os.getpid(),
        "rootUri": root_uri,
        "capabilities": {"textDocument": {"publishDiagnostics": {},
                                          "completion": {"completionItem": {}}}},
    }, timeout=300)
    lsp.notify("initialized", {})
    # A verb one side does not IMPLEMENT is a capability difference, not a
    # divergence, and conflating them buries the report: the base here does
    # not serve typeDefinition, which alone produced an `error-base` on every
    # single position. Capabilities are recorded per side and the diff
    # subtracts the asymmetry into its own section.
    caps = ((init or {}).get("result") or {}).get("capabilities") or {}
    served = sorted(v for v in VERBS if _serves(caps, v))

    # Readiness, and it has to be a CROSS-FILE gate. The workspace index is
    # lazy -- it starts on the first didOpen -- so a probe that merely
    # returns non-empty proves nothing: goto-def on a local `sub` answers
    # from the open document alone, instantly, with no index at all.
    # Measured here: the base binary passed a non-empty probe in 12 ms while
    # the head took 1,309 ms, and treating that as "both ready" would have
    # swept the base cold and reported its every unresolved cross-file
    # answer as a regression. The gate therefore requires a definition that
    # lands in a DIFFERENT FILE.
    #
    # Several candidate probes are tried in turn, because any single
    # `use Foo` may legitimately have no resolvable target in this corpus,
    # and one unlucky pick would stall the whole run at the timeout.
    probes = [p for p in pos if p["kind"] in ("use-module", "module-path")][:8] \
             or pos[:8]

    def await_cross_file(client, budget):
        """Block until a definition lands in a DIFFERENT file, or the budget
        runs out. Returns (probe_file, uri, elapsed_ms | None)."""
        t = time.monotonic()
        end = t + budget
        while time.monotonic() < end:
            for pr in probes:
                # The budget has to bind INSIDE the probe loop, not just
                # between passes. Against a wedged server every probe costs
                # its full timeout, so one pass over eight probes ran 16
                # minutes while nominally honouring a 300-second budget --
                # the run looked hung, and the budget was the thing that was
                # supposed to prove it was not.
                left = end - time.monotonic()
                if left <= 0:
                    break
                uri = _open(client, root, pr["file"])
                if uri is None:
                    continue
                msg, _ = client.request("textDocument/definition", {
                    "textDocument": {"uri": uri},
                    "position": {"line": pr["line"], "character": pr["char_on"]}},
                    timeout=min(120, left))
                locs = N.locations((msg or {}).get("result"), root_uri)
                here = os.path.relpath(os.path.join(root, pr["file"]), root)
                if any(u and u != here for (u, _) in locs):
                    return pr["file"], uri, (time.monotonic() - t) * 1000
            time.sleep(1.0)
        return None, None, None

    first, ready, ready_ms = await_cross_file(lsp, args.ready_timeout)
    if ready_ms is not None:
        ready_ms = (time.monotonic() - t_spawn) * 1000
    else:
        first = pos[0]["file"]
        ready = _open(lsp, root, first)
        print(f"[{args.side}] WARNING: no cross-file definition resolved in "
              f"{args.ready_timeout}s — sweeping anyway, and the report will "
              f"say this side was never confirmed workspace-ready", flush=True)
    if ready_ms is not None:
        print(f"[{args.side}] cross-file ready after {round(ready_ms)}ms", flush=True)

    opened, done = {first: ready}, 0
    # Lists, not ints: the loop rebinds `lsp` on restart and these have to
    # survive that without a nonlocal dance in a nested scope.
    wedge, restarts = [0], [0]
    # A re-warmed server is not the same server. Rows carry the SERVER
    # GENERATION they were answered by, and whether that generation ever
    # confirmed cross-file readiness — on Koha 3 of 8 restarts never did, and
    # a row from an unconfirmed generation is an answer from a server that
    # was, as far as anyone can show, still cold.
    epoch, epoch_warm = [0], [True]
    deferred = []
    t0 = time.monotonic()
    with open(args.out, "w") as fh:
        fh.write(json.dumps({"_meta": {
            # Identity of the QUESTIONS and of when they were asked. The diff
            # cross-checks these across all four inputs, because nothing else
            # can: four well-formed answer files from two different sweeps
            # produce a report that is entirely plausible and entirely wrong.
            # That happened — a report was generated the moment the head side
            # finished, while the two noise runs the script had yet to start
            # were still on disk from the previous invocation, and the floor
            # it published came from a different sweep.
            "positions_sha": _sha_file(args.positions),
            "positions_path": os.path.abspath(args.positions),
            "started_at": _t0_wall,
            "side": args.side, "bin": os.path.abspath(args.bin), "root": root,
            "root_uri": root_uri, "ready_ms": ready_ms and round(ready_ms),
            "cross_file_ready": ready_ms is not None, "verbs": served,
            "unserved": sorted(set(VERBS) - set(served)),
            "verb_rate": VERB_RATE, "positions": len(pos),
            "version": _version(args.bin),
        }}) + "\n")
        by_file = {}
        for p in pos:
            by_file.setdefault(p["file"], []).append(p)
        for rel, group in by_file.items():
            uri = opened.get(rel) or _open(lsp, root, rel)
            if uri is None:
                continue
            opened[rel] = uri
            # documentSymbol is per FILE, not per position -- the cheapest
            # structural signal in the sweep and worth taking on every file.
            msg, ms = lsp.request("textDocument/documentSymbol",
                                  {"textDocument": {"uri": uri}}, timeout=args.timeout)
            fh.write(json.dumps({
                "file": rel, "kind": "file", "verb": "documentSymbol",
                "ms": round(ms, 1), "epoch": epoch[0],
                **({"warm_unconfirmed": True} if not epoch_warm[0] else {}),
                "norm": N.normalize("documentSymbol", (msg or {}).get("result"), root_uri),
                "err": _err(msg),
            }, sort_keys=True, default=str) + "\n")
            for p in group:
                for verb in served:
                    method, anchor = VERBS[verb]
                    rate = VERB_RATE.get(verb, 1.0)
                    if rate < 1.0 and P.stable_frac(
                            "verb", verb, rel, p["line"], p["char_on"]) >= rate:
                        continue
                    ch = p["char_on"] if anchor == "on" else p["char_after"]
                    params = {"textDocument": {"uri": uri},
                              "position": {"line": p["line"], "character": ch}}
                    if verb == "references":
                        params["context"] = {"includeDeclaration": True}
                    msg, ms = lsp.request(method, params, timeout=args.timeout)
                    rec = {"file": rel, "line": p["line"], "char": ch,
                           "kind": p["kind"], "name": p["name"], "verb": verb,
                           "ms": round(ms, 1), "err": _err(msg),
                           "epoch": epoch[0]}
                    if not epoch_warm[0]:
                        rec["warm_unconfirmed"] = True
                    if msg is None:
                        rec["timeout"] = True
                        rec["norm"] = None
                    else:
                        rec["norm"] = N.normalize(verb, msg.get("result"), root_uri)
                    # Every empty answer is re-asked, but at the END of the
                    # run rather than here. The @INC tier resolves per module,
                    # on demand, on a background thread, so the first goto-def
                    # on `use App::Cmd::Command` can answer empty and a later
                    # one answer the file — the workspace readiness gate
                    # cannot cover that, because the module was never in the
                    # workspace. Verified by hand twice: the sweep recorded
                    # head empty on `use Storable` and on
                    # `use App::Cmd::Command` where the CLI, which starts
                    # synchronously, resolves both.
                    #
                    # An inline sleep-and-retry was the obvious fix and it was
                    # the wrong one: at 400 ms it filled ZERO of head's empties
                    # while taking the run from 51 s to 1,024 s. Too short to
                    # help, and paid on every empty answer. Deferring costs one
                    # extra request with no sleep, and asks it of the warmest
                    # server the run ever has — strictly better evidence.
                    if not rec.get("timeout") and not rec.get("err") \
                            and N.is_empty(verb, rec["norm"]):
                        deferred.append((rel, p["line"], ch, verb, method,
                                         dict(params), p["kind"], p["name"]))
                    fh.write(json.dumps(rec, sort_keys=True, default=str) + "\n")
                    # A server that stops answering ENTIRELY is the failure
                    # mode that quietly ruins a long run: every remaining
                    # request costs the full timeout, so a corpus that would
                    # sweep in two hours takes twenty and arrives as a wall
                    # of `timeout-head` that says nothing about the branch.
                    # Observed on the base binary here — client blocked,
                    # server at 0% CPU, never recovering. Respawn instead:
                    # the wedge is recorded as an EVENT (a finding in its own
                    # right) and the remaining positions still get swept.
                    if rec.get("timeout"):
                        wedge[0] += 1
                        if wedge[0] >= args.wedge_after and _alive(lsp, uri, args):
                            # The server still serves. Consecutive timeouts on
                            # one VERB are a hang cluster, not a dead process,
                            # and restarting would throw away a warm index to
                            # cure something that is not wrong. Verified
                            # independently of this driver: on the base binary
                            # a `definition` that times out at 30 s is followed
                            # by a `documentSymbol` on the same open file that
                            # answers in milliseconds.
                            fh.write(json.dumps({"_event": "hang-cluster",
                                "after_position": done, "file": rel,
                                "line": p["line"], "verb": verb,
                                "consecutive_timeouts": wedge[0]}) + "\n")
                            fh.flush()
                            wedge[0] = 0
                        elif wedge[0] >= args.wedge_after:
                            restarts[0] += 1
                            fh.write(json.dumps({"_event": "server-wedged",
                                "after_position": done, "file": rel,
                                "line": p["line"], "verb": verb,
                                "consecutive_timeouts": wedge[0],
                                "restart": restarts[0]}) + "\n")
                            fh.flush()
                            print(f"[{args.side}] server wedged at {rel}:{p['line']} "
                                  f"({verb}); restart #{restarts[0]}", flush=True)
                            if restarts[0] > args.max_restarts:
                                fh.write(json.dumps({"_event": "aborted",
                                    "reason": "restart budget exhausted",
                                    "swept": done, "of": len(pos)}) + "\n")
                                print(f"[{args.side}] ABORTED after "
                                      f"{restarts[0]} restarts", flush=True)
                                lsp.shutdown()
                                return
                            try:
                                lsp.proc.kill()
                            except Exception:
                                pass
                            lsp = spawn()
                            lsp.request("initialize", {
                                "processId": os.getpid(), "rootUri": root_uri,
                                "capabilities": {"textDocument": {
                                    "publishDiagnostics": {},
                                    "completion": {"completionItem": {}}}}},
                                timeout=300)
                            lsp.notify("initialized", {})
                            opened.clear()
                            # A respawned server is COLD. Sweeping on before
                            # it re-warms records its unresolved cross-file
                            # answers as lost resolutions -- the same trap
                            # the initial gate exists to close, and easier to
                            # miss here because the run is already underway.
                            _, _, warm_ms = await_cross_file(lsp, args.rewarm_timeout)
                            epoch[0] += 1
                            epoch_warm[0] = warm_ms is not None
                            fh.write(json.dumps({"_event": "restart-rewarm",
                                "restart": restarts[0], "epoch": epoch[0],
                                "cross_file_ready_ms": warm_ms and round(warm_ms),
                                "confirmed": warm_ms is not None}) + "\n")
                            if warm_ms is None:
                                print(f"[{args.side}] WARNING: restart "
                                      f"#{restarts[0]} never re-reached "
                                      f"cross-file readiness", flush=True)
                            uri = _open(lsp, root, rel)
                            opened[rel] = uri
                            wedge[0] = 0
                    else:
                        wedge[0] = 0
                done += 1
                if done % 250 == 0:
                    fh.flush()
                    el = time.monotonic() - t0
                    print(f"[{args.side}] {done}/{len(pos)} positions  {el:.0f}s "
                          f"({done/max(el,1e-9):.1f}/s)", flush=True)
        # Second pass. Recheck rows are APPENDED, not rewritten in place, so
        # the stream stays crash-resilient and both answers survive: the
        # report can say a position was empty on first ask and resolved once
        # warm, which is a fact about startup, not about the branch.
        filled = 0
        for (rel, ln, ch, verb, method, params, kind, name) in deferred:
            msg, ms = lsp.request(method, params, timeout=args.timeout)
            if msg is None or _err(msg):
                continue
            norm = N.normalize(verb, msg.get("result"), root_uri)
            if N.is_empty(verb, norm):
                continue
            filled += 1
            fh.write(json.dumps({"_recheck": True, "file": rel, "line": ln,
                                 "char": ch, "verb": verb, "kind": kind,
                                 "name": name, "ms": round(ms, 1),
                                 "epoch": epoch[0], "norm": norm},
                                sort_keys=True, default=str) + "\n")
        fh.write(json.dumps({"_event": "recheck",
                             "empty_first_ask": len(deferred),
                             "filled_when_warm": filled}) + "\n")
        print(f"[{args.side}] recheck: {filled} of {len(deferred)} empty answers "
              f"resolved once warm", flush=True)
    lsp.shutdown()
    print(f"[{args.side}] done: {done} positions in "
          f"{time.monotonic()-t0:.0f}s -> {args.out}")


CAP_KEY = {
    "definition": "definitionProvider",
    "typeDefinition": "typeDefinitionProvider",
    "hover": "hoverProvider",
    "references": "referencesProvider",
    "completion": "completionProvider",
}


def _serves(caps, verb):
    """Whether the server advertised the verb at `initialize`.

    A provider may be `true`, an options object, or absent; `false` and
    absent both mean unserved. Note the one thing this cannot see: a verb
    registered DYNAMICALLY (typeHierarchy is, here) is absent from the
    static capabilities and would be skipped. That is the safe direction --
    a skipped verb is visible in `unserved`, whereas a swept-but-unimplemented
    verb silently becomes thousands of fake regressions.
    """
    v = caps.get(CAP_KEY[verb])
    return bool(v) if not isinstance(v, dict) else True


def _version(binp):
    try:
        return subprocess.run([binp, "--version"], capture_output=True,
                              text=True, timeout=60).stdout.strip()
    except Exception as e:
        return f"<unavailable: {e}>"


def _alive(client, uri, args):
    """Is the server still serving, or is the process itself gone?

    Asked with `documentSymbol` on an ALREADY-OPEN file: it needs no
    cross-file resolution and no index, so an answer means the process reads
    stdin and writes stdout. Anything more ambitious would confuse "slow to
    resolve" with "dead", which is the distinction this call exists to make.
    """
    msg, _ = client.request("textDocument/documentSymbol",
                            {"textDocument": {"uri": uri}},
                            timeout=min(10.0, args.timeout))
    return msg is not None


def _err(msg):
    if msg and "error" in msg:
        return str(msg["error"].get("message", msg["error"]))[:200]
    return None


def _open(lsp, root, rel):
    try:
        text = open(os.path.join(root, rel), encoding="utf8", errors="replace").read()
    except OSError:
        return None
    uri = pathlib.Path(os.path.join(root, rel)).as_uri()
    lang = "perl"
    lsp.notify("textDocument/didOpen", {"textDocument": {
        "uri": uri, "languageId": lang, "version": 1, "text": text}})
    return uri


# ---- diff ----

def _load(path):
    meta, rows, events = {}, {}, []
    for line in open(path):
        r = json.loads(line)
        if "_meta" in r:
            meta = r["_meta"]
            continue
        if "_event" in r:
            events.append(r)
            continue
        key = (r["file"], r.get("line"), r.get("char"), r["verb"])
        if r.get("_recheck"):
            # A warm answer supersedes a cold empty one. Both are in the file;
            # this is the choice of which the comparison uses, and it must be
            # the warm one on BOTH sides or the sweep measures startup.
            if key in rows:
                rows[key] = dict(rows[key], norm=r["norm"],
                                 filled_when_warm=True)
            continue
        rows[key] = r
    meta["events"] = events
    return meta, rows


def classify(verb, a, b):
    """One position, one verb, two sides -> a divergence SHAPE.

    The shapes are the whole point of the report. `only-head` and `only-base`
    are the two that carry a verdict on their face -- one side resolves
    something the other does not -- and `superset` / `subset` say the same
    thing with the weaker claim that both found something. `disagree` is the
    residual that always needs a human.
    """
    ta, tb = a.get("timeout"), b.get("timeout")
    if ta or tb:
        return "timeout-base" if ta and not tb else ("timeout-head" if tb and not ta else "timeout-both")
    if a.get("err") or b.get("err"):
        if a.get("err") and b.get("err"):
            return "same" if a["err"] == b["err"] else "error-differs"
        return "error-base" if a.get("err") else "error-head"
    na, nb = a.get("norm"), b.get("norm")
    if N.answer_key(verb, na) == N.answer_key(verb, nb):
        return "same"
    ea, eb = N.is_empty(verb, na), N.is_empty(verb, nb)
    if ea and not eb:
        return "only-head"
    if eb and not ea:
        return "only-base"
    sa, sb = N.as_set(verb, na), N.as_set(verb, nb)
    if sa is not None and sb is not None and sa != sb:
        # A truncated list is a strict subset of an untruncated one BY
        # DESIGN. Ranking on one side and not the other would otherwise
        # report every long completion as a lost-candidates regression --
        # 221 rows in the first corpus run, all of them one intended change.
        trunc = lambda n: verb == "completion" and isinstance(n, dict) \
                          and n.get("incomplete")
        if sb < sa and trunc(nb):
            return "capped-head"
        if sa < sb and trunc(na):
            return "capped-base"
        if sa < sb:
            return "superset"
        if sb < sa:
            return "subset"
        return "disagree"
    # Set-equal but key-different: completion whose RANKING moved, or a
    # hover whose text changed. Naming it separately is what keeps a ranking
    # regression from hiding behind an unchanged candidate set.
    if verb == "completion" and sa == sb:
        return "reranked"
    return "content-differs"


SHAPE_MEANING = {
    "only-head":       "head answers, base empty  (new resolution -- intended improvement?)",
    "only-base":       "base answers, head empty  (LOST resolution -- regression candidate)",
    "superset":        "head found everything base did, plus more",
    "subset":          "head found strictly fewer  (regression candidate)",
    "capped-head":     "head's list is a subset because head TRUNCATED it (isIncomplete) — by design, not a loss",
    "capped-base":     "base's list is a subset because base truncated it — by design, not a loss",
    "disagree":        "both non-empty, neither contains the other",
    "reranked":        "same candidates, different order (completion ranking moved)",
    "content-differs": "same shape, different content (hover text, etc.)",
    "error-head":      "head returned a protocol error, base did not",
    "error-base":      "base returned a protocol error, head did not",
    "error-differs":   "both errored, differently",
    "timeout-head":    "head timed out, base answered",
    "timeout-base":    "base timed out, head answered",
    "timeout-both":    "both timed out",
    "missing-head":    "position present in base run only",
    "missing-base":    "position present in head run only",
}
# Ordering of the report: the buckets a reviewer must adjudicate first.
SHAPE_ORDER = ["only-base", "subset", "timeout-head", "error-head", "disagree",
               "content-differs", "reranked", "only-head", "superset",
               "capped-head", "capped-base",
               "timeout-base", "error-base", "error-differs", "timeout-both",
               "missing-head", "missing-base"]


def _shape_counts(base_path, head_path, common, only_keys):
    """The noise floor, measured over EXACTLY the answers the A/B compared.

    `only_keys` is not an optimisation, it is the whole correctness of the
    number. The floor is a per-answer rate, so it is only subtractable from a
    count taken over the same answers. Measured on Koha: the base wedged
    repeatedly and produced 1,184 comparable answers where the two noise runs
    (which never wedge, being the head binary) produced ~21,790 — quoting a
    floor from 21,790 beside a count from 1,184 compares populations that
    differ by a factor of eighteen and share an unknown fraction of their
    members.

    It is also biased, not merely scaled: the answers the base reached before
    wedging are the CHEAP ones, and cheap positions are exactly where two
    runs agree. A whole-corpus floor is therefore an over-estimate of the
    floor on the surviving sample in some shapes and an under-estimate in
    others, with no way to tell which from the number itself.

    Returns the histogram plus how many of `only_keys` the noise runs could
    actually answer — a floor measured over a fraction of the comparison is
    still not comparable, and the caller has to be able to say so.
    """
    _, a = _load(base_path)
    _, b = _load(head_path)
    keys = (set(a) & set(b) & only_keys)
    out = {}
    for k in keys:
        if k[3] not in common:
            continue
        sh = classify(k[3], a[k], b[k])
        if sh != "same":
            # Keyed by (shape, verb) as well as shape. An aggregate floor is
            # the wrong baseline for a single-verb block and the error is not
            # conservative in either direction: measured here, `disagree` is
            # 5 aggregate and ALL 5 of it is completion, so a `definition`
            # sub-block read against the aggregate is handed noise it does
            # not have, while on another corpus the reverse hides real signal.
            out[sh] = out.get(sh, 0) + 1
            out[(sh, k[3])] = out.get((sh, k[3]), 0) + 1
    return out, len(keys)


def _check_provenance(meta_a, meta_b, args):
    """Do all four inputs come from ONE sweep, asking ONE set of questions?

    Nothing downstream can tell that they do not. Four well-formed answer
    files produce a well-formed report whether or not they belong together,
    and the failure is silent and plausible — a floor from a previous run,
    quoted beside fresh counts, reads exactly like a floor from this one.
    It happened here, and it published wrong numbers.

    Two checks. The positions hash must match, because a floor over different
    QUESTIONS is not a floor at all. And the noise runs must not predate the
    A/B: a noise pair older than the run it is quoted against is the stale
    -file case by definition.
    """
    metas = {}
    for label, path in (("noise-base", args.noise_base),
                        ("noise-head", args.noise_head)):
        m, _ = _load(path)
        metas[label] = m
    want = meta_b.get("positions_sha") or meta_a.get("positions_sha")
    if want:
        for label, m in metas.items():
            got = m.get("positions_sha")
            if got and got != want:
                return (f"{label} answered a DIFFERENT position set "
                        f"({got} vs {want}) — its floor is not a floor for "
                        f"this comparison")
            if not got:
                return (f"{label} predates position-set stamping, so it "
                        f"cannot be shown to answer the same questions")
    ab_start = max(x for x in (meta_a.get("started_at"), meta_b.get("started_at"))
                   if x) if (meta_a.get("started_at") or meta_b.get("started_at")) else None
    if ab_start:
        for label, m in metas.items():
            st = m.get("started_at")
            if st and st < ab_start:
                return (f"{label} STARTED BEFORE the A/B run it is quoted "
                        f"against — this is the stale-file case; re-run the "
                        f"noise pair or pass --force-noise")
    return None


def cmd_diff(args):
    meta_a, base = _load(args.base)
    meta_b, head = _load(args.head)

    # Verbs only one side serves are subtracted BEFORE anything is counted.
    # Leaving them in would put a capability gap at the top of the
    # regression list on every position that has one, which is the fastest
    # way to make a divergence report unreadable.
    va, vb = set(meta_a.get("verbs") or []), set(meta_b.get("verbs") or [])
    common = (va & vb) | {"documentSymbol"}
    cap_only = sorted((va | vb) - common)
    # Only positions BOTH sides answered are comparable. A side that aborted
    # early (the base wedges often enough here that it can) would otherwise
    # contribute thousands of `missing-base` rows that say nothing about the
    # branch and bury what does. The shortfall is reported as COVERAGE
    # instead, which is the honest framing: this is what we compared, and
    # this is what we could not.
    all_keys = {k for k in (set(base) | set(head)) if k[3] in common}
    comparable = {k for k in all_keys if k in base and k in head}
    only_base_keys = sum(1 for k in all_keys if k not in head)
    only_head_keys = sum(1 for k in all_keys if k not in base)

    # An answer from a server generation that never re-confirmed cross-file
    # readiness is an answer from a server that was, as far as anything here
    # can show, still cold. It is held out rather than counted: an unconfirmed
    # empty is indistinguishable from a lost resolution, which is the one
    # confusion this whole harness exists to prevent.
    unconfirmed = {k for k in comparable
                   if base[k].get("warm_unconfirmed")
                   or head[k].get("warm_unconfirmed")}
    keys = comparable - unconfirmed
    # Rows answered AFTER a restart by a generation that did re-warm are
    # counted, but reported: they came from a different process with a
    # rebuilt index, and a reader deciding how much to trust a thin block
    # should know how much of it is post-restart.
    post_restart = sum(1 for k in keys
                       if base[k].get("epoch", 0) or head[k].get("epoch", 0))

    groups, counts, uninformative = {}, {}, 0
    for k in sorted(keys, key=lambda k: (k[0], k[3], k[1] is None, k[1] or 0, k[2] or 0)):
        a, b = base[k], head[k]
        verb = k[3]
        shape = classify(verb, a, b)
        if shape == "same":
            # A position where both sides answered nothing on every verb is
            # the residue of an imperfect tokenizer (a name inside a string,
            # say). Counting it as agreement would inflate the denominator
            # with questions nobody asked; it is reported separately instead.
            if N.is_empty(verb, a.get("norm")) and N.is_empty(verb, b.get("norm")):
                uninformative += 1
            counts[("same", verb)] = counts.get(("same", verb), 0) + 1
            continue
        kind = (b or a).get("kind", "?")
        counts[(shape, verb)] = counts.get((shape, verb), 0) + 1
        groups.setdefault((shape, verb, kind), []).append((k, a, b))

    # The noise floor: the SAME binary against itself over the same
    # positions. Measured here it is 3.1% of answers -- 162 `reranked` and 11
    # `disagree`, and ZERO in every other shape. Without it a reader has no
    # way to tell a 68-row `reranked` block from nothing at all, because
    # completion ordering is not stable run to run. This is not a correction
    # applied to the counts; it is the resolution limit printed beside them.
    noise, noise_n, noise_reject = {}, 0, None
    if args.noise_base and args.noise_head:
        noise_reject = _check_provenance(meta_a, meta_b, args)
        if noise_reject:
            print(f"WARNING: {noise_reject}", file=sys.stderr)
        if not (noise_reject and not args.force_noise):
            noise, noise_n = _shape_counts(args.noise_base, args.noise_head,
                                           common, keys)

    total = len(keys)
    diverged = sum(v for (s, _), v in counts.items() if s != "same")
    same = total - diverged

    out = []
    W = out.append
    W("# Differential sweep report\n")
    W(f"- **base** `{meta_a.get('side','base')}` — `{meta_a.get('version','?')}`")
    W(f"- **head** `{meta_b.get('side','head')}` — `{meta_b.get('version','?')}`")
    W(f"- corpus: `{meta_b.get('root', meta_a.get('root','?'))}`")
    W(f"- path: **server** (LSP over stdio), verbs "
      f"{', '.join(sorted(common))}")
    if cap_only:
        W(f"- **excluded as a capability difference:** "
          + ", ".join(f"`{v}`" for v in cap_only)
          + " — served by one side only, so every position would report a "
            "divergence that is a missing feature rather than a changed answer")
    rate = meta_b.get("verb_rate") or {}
    if rate:
        W(f"- sampled verbs: " + ", ".join(f"`{k}` at {v:.0%}" for k, v in sorted(rate.items())))
    W(f"- cross-file readiness: base {meta_a.get('ready_ms')} ms, "
      f"head {meta_b.get('ready_ms')} ms")
    for side, m in (("base", meta_a), ("head", meta_b)):
        if not m.get("cross_file_ready", True):
            W(f"- **{side} WAS NEVER CONFIRMED WORKSPACE-READY** — no cross-file "
              f"definition resolved within the readiness budget, so this side's "
              f"empty answers may be coldness rather than disagreement. Treat "
              f"every `only-*` shape below as unattributed until this is fixed.")
    for side, m in (("base", meta_a), ("head", meta_b)):
        for ev in m.get("events") or []:
            W(f"- **{side} {ev['_event']}**: " + ", ".join(
                f"{k}={v}" for k, v in sorted(ev.items()) if k != "_event"))
    W("")
    W(f"**{total} (position, verb) answers compared — {same} identical "
      f"({same/max(total,1):.2%}), {diverged} divergent.**\n")
    if only_base_keys or only_head_keys:
        reach = total / max(len(all_keys), 1)
        W(f"Coverage shortfall: {only_base_keys} answers exist only in the base "
          f"run and {only_head_keys} only in the head run — a side that aborted "
          f"or skipped never produced them. They are EXCLUDED from every count "
          f"above, because a position one side never reached is a gap in the "
          f"sweep, not a disagreement about an answer.\n")
        if reach < 0.5:
            W(f"> **Only {reach:.1%} of positions were answered by both sides.** "
              f"The comparable set is not a random sample of the corpus — it is "
              f"the positions the weaker side got through before it stopped, "
              f"which are the cheap ones. Treat what follows as a statement "
              f"about coverage, not a divergence list.\n")
    if unconfirmed:
        W(f"Held out: {len(unconfirmed)} answers came from a server generation "
          f"that never re-confirmed cross-file readiness after a restart. They "
          f"are excluded from every count — an unconfirmed empty cannot be "
          f"told apart from a lost resolution.\n")
    if post_restart:
        W(f"Of the {total} compared, {post_restart} were answered after at "
          f"least one side restarted (by a generation that did re-warm). A "
          f"rebuilt index is not the same index.\n")
    W(f"Of the identical ones, {uninformative} were empty on both sides: positions "
      f"nobody would ask about, kept in the denominator but called out so the "
      f"agreement rate is not read as coverage.\n")

    W("## Divergences by shape\n")
    by_shape = {}
    for (s, v), n in counts.items():
        if s != "same":
            by_shape[s] = by_shape.get(s, 0) + n
    if noise_reject:
        W(f"> **NOISE FLOOR REJECTED — {noise_reject}.** No floor is shown "
          f"below. Counts are raw, and `reranked` in particular cannot be "
          f"read without one.\n" if not args.force_noise else
          f"> **Noise floor is SUSPECT — {noise_reject}.** Shown anyway "
          f"because `--force-noise` was passed.\n")
    if noise or noise_n:
        W("`noise` is the same shape's count when the SAME binary is run "
          "twice, measured over EXACTLY the answers compared here — a rate "
          "taken over a different population is not subtractable from this "
          "one. A block at or below its floor carries no information: read "
          "`signal`, not `n`.\n")
        cover = noise_n / max(total, 1)
        if cover < 0.98:
            W(f"> **The floor covers {noise_n} of {total} compared answers "
              f"({cover:.1%}).** The rest could not be measured: the noise "
              f"runs did not answer at those positions. Shapes below are "
              f"scaled by that fraction only if the unmeasured answers behave "
              f"like the measured ones, which is exactly what a run that "
              f"stalled part-way does not guarantee.\n")
        W("| shape | n | noise | signal | meaning |")
        W("|---|---|---|---|---|")
        for s in SHAPE_ORDER:
            if by_shape.get(s):
                nf = noise.get(s, 0)
                sig = "**below noise — unreadable**" if by_shape[s] <= nf \
                      else f"{by_shape[s]-nf}"
                W(f"| `{s}` | {by_shape[s]} | {nf} | {sig} | {SHAPE_MEANING.get(s,'')} |")
    else:
        W("*No noise floor supplied (`--noise-base` / `--noise-head`). Completion "
          "ordering is not stable run to run, so `reranked` counts in particular "
          "cannot be interpreted without one.*\n")
        W("| shape | n | meaning |")
        W("|---|---|---|")
        for s in SHAPE_ORDER:
            if by_shape.get(s):
                W(f"| `{s}` | {by_shape[s]} | {SHAPE_MEANING.get(s,'')} |")
    W("")

    W("## Groups\n")
    W("Each row is one claim to adjudicate: *intended improvement*, "
      "*regression*, or *wash*.\n")
    W("`distinct` is the number of different (base answer, head answer) PAIRS "
      "behind the positions — the count of separate claims. One generated "
      "data file can contribute sixty positions that all disagree the same "
      "way; that is one thing to adjudicate, not sixty, and reading `n` as "
      "the workload is how a sweep gets abandoned as noise.\n")
    if noise:
        W("The `verb noise` column is the floor for that shape ON THAT VERB, "
          "which is the only baseline a single-verb block can be read "
          "against. It is summed over the verb's kinds, so a block covering "
          "one kind sits well under it.\n")
    W("| shape | verb | token kind | n | distinct |"
      + (" verb noise |" if noise else ""))
    W("|---|---|---|---|---|" + ("---|" if noise else ""))
    for (shape, verb, kind), items in sorted(
            groups.items(), key=lambda kv: (SHAPE_ORDER.index(kv[0][0])
                                            if kv[0][0] in SHAPE_ORDER else 99,
                                            -len(kv[1]))):
        row = (f"| `{shape}` | {verb} | {kind} | {len(items)} | "
               f"{_distinct(verb, items)} |")
        if noise:
            row += f" {noise.get((shape, verb), 0)} |"
        W(row)
    W("")

    W("## Examples\n")
    for (shape, verb, kind), items in sorted(
            groups.items(), key=lambda kv: (SHAPE_ORDER.index(kv[0][0])
                                            if kv[0][0] in SHAPE_ORDER else 99,
                                            -len(kv[1]))):
        reps = _representatives(verb, items)
        W(f"### `{shape}` · {verb} · {kind} — {len(items)} positions, "
          f"{len(reps)} distinct\n")
        for (k, a, b), n_same in reps[:args.examples]:
            f, ln, ch, _ = k
            more = f"  (+{n_same-1} more positions answering identically)" if n_same > 1 else ""
            W(f"- `{f}:{ln}:{ch}` `{(b or a).get('name','?')}`{more}")
            W(f"  - base: `{_brief(verb, a.get('norm'))}`")
            W(f"  - head: `{_brief(verb, b.get('norm'))}`")
        if len(reps) > args.examples:
            W(f"- …and {len(reps)-args.examples} more distinct claims")
        W("")

    txt = "\n".join(out)
    if args.out:
        open(args.out, "w").write(txt)
        print(f"report -> {args.out}  ({diverged} divergent of {total})")
    else:
        print(txt)


def _pair_key(verb, a, b):
    return (N.answer_key(verb, a.get("norm")), N.answer_key(verb, b.get("norm")))


def _distinct(verb, items):
    return len({_pair_key(verb, a, b) for (_, a, b) in items})


def _representatives(verb, items):
    """One example per DISTINCT (base, head) answer pair, largest cluster first.

    Showing the first N positions instead would print the same disagreement
    N times whenever a group is dominated by one repeated answer — which is
    the common case, not the exception.
    """
    seen = {}
    for it in items:
        seen.setdefault(_pair_key(verb, it[1], it[2]), []).append(it)
    out = [(v[0], len(v)) for v in seen.values()]
    out.sort(key=lambda t: -t[1])
    return out


def _brief(verb, norm, cap=200):
    if norm is None:
        return "∅"
    if verb == "completion":
        return f"n={norm.get('n')} top={norm.get('top', [])[:4]}"
    if verb == "hover":
        return f"len={norm.get('len')} {norm.get('head','')[:120]!r}"
    s = json.dumps(norm, default=str)
    return s if len(s) <= cap else s[:cap] + "…"


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("positions")
    p.add_argument("--root", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--seed", default="v1")
    p.add_argument("--per-file", type=int, default=25)
    p.add_argument("--max-files", type=int, default=0)
    p.set_defaults(fn=cmd_positions)

    p = sub.add_parser("run")
    p.add_argument("--bin", required=True)
    p.add_argument("--root", required=True)
    p.add_argument("--positions", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--side", default="run")
    p.add_argument("--cache-dir", required=True)
    p.add_argument("--timeout", type=float, default=30.0)
    p.add_argument("--ready-timeout", type=float, default=600.0)
    p.add_argument("--rewarm-timeout", type=float, default=60.0,
                   help="budget for re-reaching cross-file readiness after a restart")
    p.add_argument("--wedge-after", type=int, default=3,
                   help="consecutive timeouts that mean the server is wedged")
    p.add_argument("--max-restarts", type=int, default=10)
    p.set_defaults(fn=cmd_run)

    p = sub.add_parser("diff")
    p.add_argument("--base", required=True)
    p.add_argument("--head", required=True)
    p.add_argument("--out")
    p.add_argument("--examples", type=int, default=8)
    p.add_argument("--noise-base", help="answers from a repeat run of ONE binary")
    p.add_argument("--noise-head", help="answers from a second run of that SAME binary")
    p.add_argument("--force-noise", action="store_true",
                   help="use the noise pair even if it fails the provenance check")
    p.set_defaults(fn=cmd_diff)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
