#!/usr/bin/env python3
"""Append KPI baseline rows to bench/baselines.jsonl from a harness run.

  bench/seed-baselines.py <run.jsonl> [--note "why this run is trusted"]

Baselines are CURATED: run this deliberately after a sweep you trust, review
the diff, commit it. Never wired into collection — an auto-appended baseline
is just a moving average wearing a baseline's clothes.

What qualifies as a KPI lives here, on purpose, in one place:
  batch : check.wall + peak RSS (cold and warm medians), per-file build
          p50/p99/max — the leading indicator that catches the next
          20-second file before a user does.
  editor: startup ready, per-verb p50/p99, diagnostics push, server RSS
          (from lsp_bench --jsonl rows, when the run contains them).
Counters and per-file lanes are DIAGNOSTICS, never baselined: they exist to
attribute a KPI move and legitimately shift with every refactor.

Every row carries (sha, dirty, date, host, n, spread): a baseline that can't
say how noisy it was can't support a regression claim, and one from a dirty
tree says so to your face.
"""
import json, statistics as st, sys
from collections import defaultdict

def main():
    path = sys.argv[1]
    note = ""
    if "--note" in sys.argv:
        note = sys.argv[sys.argv.index("--note") + 1]
    run = None
    vals = defaultdict(list)          # (corpus, phase, metric) -> samples
    files = defaultdict(list)         # (corpus, phase) -> per-file build ms
    for line in open(path):
        r = json.loads(line)
        if r.get("t") == "run":
            run = r
            continue
        if r.get("t") != "m":
            continue
        k, c, p = r["kind"], r["corpus"], r["phase"]
        if k == "timing" and r["name"] == "check.wall":
            vals[(c, p, "check.wall_ms")].append(r["value"])
        elif k == "rss" and r["name"] == "peak":
            vals[(c, p, "check.peak_rss_mb")].append(r["value"])
        elif k == "file_build":
            files[(c, p)].append(r["value"])
        elif k == "startup":
            vals[(c, p, f"editor.{r['name']}_ms")].append(r["value"])
        elif k == "verb_ms":
            vals[(c, p, f"editor.verb.{r['name']}_ms")].append(r["value"])
        elif k == "diag_push_ms":
            vals[(c, p, "editor.diag_push_ms")].append(r["value"])
        elif k == "server_rss" and r["name"] == "end":
            vals[(c, p, "editor.rss_end_mb")].append(r["value"])
    assert run, "no run line in input"

    out = []
    def row(corpus, phase, metric, samples, stat="median"):
        if not samples:
            return
        v = {"median": st.median(samples),
             "p99": sorted(samples)[min(len(samples) - 1, int(len(samples) * 0.99))],
             "max": max(samples)}[stat]
        out.append({
            "date": run["ts"][:10], "sha": run["sha"], "dirty": run.get("dirty"),
            "host": run.get("host"), "corpus": corpus, "phase": phase,
            "metric": metric if stat == "median" else f"{metric}.{stat}",
            "value": round(v, 2), "n": len(samples),
            "spread": round(max(samples) - min(samples), 2),
            "note": note,
        })
    for (c, p, mname), samples in sorted(vals.items()):
        row(c, p, mname, samples)
    for (c, p), samples in sorted(files.items()):
        for stat in ("median", "p99", "max"):
            row(c, p, "file_build_ms", samples, stat)

    with open("bench/baselines.jsonl", "a") as f:
        for r in out:
            f.write(json.dumps(r) + "\n")
    print(f"appended {len(out)} baseline rows from {run['run_id']} (sha {run['sha']}, dirty={run.get('dirty')})")

if __name__ == "__main__":
    main()
