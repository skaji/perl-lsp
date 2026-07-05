#!/usr/bin/env python3
"""Minimal LSP-protocol latency + peak-RSS probe. LSP-agnostic: point --bin at
any stdio LSP server (perl-lsp or clangd) and it drives the same handshake +
query sequence, timing each hop from process spawn. No editor, no deps beyond
stdlib. Peak RSS is read from /proc/<pid>/status VmHWM right before shutdown
(the kernel tracks the historical peak for us — no polling thread needed).

Usage:
  lsp_latency.py --bin <exe> [--arg X ...] --root <dir> --file <path>
                  --line <1-based> --col <1-based>
                  --query {definition,references,hover} [--warm-repeat N]
                  [--retry-until-non-null] [--retry-timeout-secs S]
                  [--wait-indexing-secs S] [--dump-full-response PATH]

Prints one JSON object to stdout with timing + peak-RSS + response summaries.
Not wired into e2e/run.sh — opt-in only, for perf/comparison scouting
(see docs/clangd-benchmark-procedure.md). ASCII/1-byte-UTF-8 positions only:
character offsets are sent as a raw char index, which only matches the LSP
UTF-16 code-unit convention for ASCII source (true of every fixture/corpus
this has been pointed at so far).
"""
import argparse
import json
import os
import subprocess
import sys
import threading
import time
import urllib.parse


def read_message(stream):
    headers = {}
    while True:
        line = stream.readline()
        if not line:
            return None
        line = line.decode("utf-8", "replace").rstrip("\r\n")
        if line == "":
            break
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    length = int(headers.get("content-length", 0))
    if length == 0:
        return None
    body = stream.read(length)
    return json.loads(body.decode("utf-8"))


def write_message(stream, obj):
    body = json.dumps(obj).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8")
    stream.write(header)
    stream.write(body)
    stream.flush()


class Client:
    def __init__(self, proc):
        self.proc = proc
        self._id = 0
        self._lock = threading.Lock()
        self._pending = {}
        self._notifications = []
        self._reader = threading.Thread(target=self._read_loop, daemon=True)
        self._reader.start()

    def _read_loop(self):
        while True:
            try:
                msg = read_message(self.proc.stdout)
            except Exception:
                return
            if msg is None:
                return
            now = time.monotonic()
            if "id" in msg and ("result" in msg or "error" in msg):
                with self._lock:
                    self._pending[msg["id"]] = (now, msg)
            elif "id" in msg and "method" in msg:
                # Server-initiated request (workDoneProgress/create, registerCapability,
                # configuration, …). Auto-ack with a null result so the server doesn't
                # stall waiting on a real editor's response.
                with self._lock:
                    self._notifications.append((now, msg))
                try:
                    write_message(self.proc.stdin, {"jsonrpc": "2.0", "id": msg["id"], "result": None})
                except Exception:
                    pass
            else:
                with self._lock:
                    self._notifications.append((now, msg))

    def next_id(self):
        self._id += 1
        return self._id

    def request(self, method, params, timeout=60):
        mid = self.next_id()
        write_message(self.proc.stdin, {
            "jsonrpc": "2.0", "id": mid, "method": method, "params": params,
        })
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                if mid in self._pending:
                    return self._pending.pop(mid)
            time.sleep(0.005)
        raise TimeoutError(f"no response to {method} within {timeout}s")

    def notify(self, method, params):
        write_message(self.proc.stdin, {
            "jsonrpc": "2.0", "method": method, "params": params,
        })

    def notifications_matching(self, pred):
        with self._lock:
            return [(t, m) for (t, m) in self._notifications if pred(m)]


def to_uri(path):
    return "file://" + urllib.parse.quote(os.path.abspath(path))


def peak_rss_kb(pid):
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1])
    except FileNotFoundError:
        pass
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--arg", action="append", default=[])
    ap.add_argument("--root", required=True)
    ap.add_argument("--file", required=True)
    ap.add_argument("--line", type=int, required=True, help="1-based")
    ap.add_argument("--col", type=int, required=True, help="1-based")
    ap.add_argument("--query", choices=["definition", "references", "hover"], default="definition")
    ap.add_argument("--warm-repeat", type=int, default=1)
    ap.add_argument("--wait-indexing-secs", type=float, default=0.0,
                     help="after first query, sleep this long (letting background index/preamble build settle) before the warm repeat and before reading peak RSS")
    ap.add_argument("--retry-until-non-null", action="store_true",
                     help="if the first query result is null/empty, keep retrying (200ms interval, up to --retry-timeout-secs) and report when it first turns non-null — the real time-to-correct-answer past any degraded-window null")
    ap.add_argument("--retry-timeout-secs", type=float, default=30.0)
    ap.add_argument("--dump-full-response", metavar="PATH", default=None,
                     help="write the full final (healed if applicable) response JSON to this path, for completeness/file-breakdown analysis")
    args = ap.parse_args()

    t_spawn = time.monotonic()
    proc = subprocess.Popen(
        [args.bin, *args.arg],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    )
    client = Client(proc)

    root_uri = to_uri(args.root)
    _, init_msg = client.request("initialize", {
        "processId": os.getpid(),
        "rootUri": root_uri,
        "capabilities": {
            "window": {"workDoneProgress": True},
            "textDocument": {
                "definition": {"linkSupport": False},
                "references": {},
                "hover": {"contentFormat": ["plaintext", "markdown"]},
            }
        },
        "workspaceFolders": [{"uri": root_uri, "name": "root"}],
    })
    t_initialized = time.monotonic()
    client.notify("initialized", {})

    file_path = args.file if os.path.isabs(args.file) else os.path.join(args.root, args.file)
    with open(file_path, "r", errors="replace") as f:
        text = f.read()
    file_uri = to_uri(file_path)
    client.notify("textDocument/didOpen", {
        "textDocument": {
            "uri": file_uri, "languageId": "cpp", "version": 1, "text": text,
        }
    })

    pos = {"line": args.line - 1, "character": args.col - 1}
    method = {
        "definition": "textDocument/definition",
        "references": "textDocument/references",
        "hover": "textDocument/hover",
    }[args.query]
    params = {"textDocument": {"uri": file_uri}, "position": pos}
    if args.query == "references":
        params["context"] = {"includeDeclaration": True}

    def is_empty(resp):
        r = resp.get("result")
        if r is None:
            return True
        if isinstance(r, list) and len(r) == 0:
            return True
        return False

    t_before_first = time.monotonic()
    _, first_resp = client.request(method, params, timeout=120)
    t_first = time.monotonic()

    raw_first_resp = first_resp
    healed_ms = None
    healed_resp = None
    if args.retry_until_non_null and is_empty(first_resp):
        deadline = time.monotonic() + args.retry_timeout_secs
        while time.monotonic() < deadline:
            time.sleep(0.2)
            _, retry_resp = client.request(method, params, timeout=30)
            if not is_empty(retry_resp):
                healed_ms = round((time.monotonic() - t_spawn) * 1000, 1)
                healed_resp = retry_resp
                first_resp = retry_resp  # used for warm-repeat baseline / sanity
                break

    if args.wait_indexing_secs > 0:
        time.sleep(args.wait_indexing_secs)

    warm_latencies = []
    warm_resp = None
    for _ in range(args.warm_repeat):
        t0 = time.monotonic()
        _, warm_resp = client.request(method, params, timeout=120)
        warm_latencies.append(time.monotonic() - t0)

    rss = peak_rss_kb(proc.pid)

    progress = client.notifications_matching(lambda m: m.get("method") == "$/progress")
    progress_summary = []
    for t, m in progress:
        v = m.get("params", {}).get("value", {})
        progress_summary.append({
            "t_ms_since_spawn": round((t - t_spawn) * 1000, 1),
            "kind": v.get("kind"),
            "title": v.get("title"),
            "message": v.get("message"),
            "percentage": v.get("percentage"),
        })

    def summarize(resp):
        r = resp.get("result")
        if r is None:
            return {"kind": "null"}
        if isinstance(r, list):
            return {"kind": "list", "count": len(r), "sample": r[:3]}
        if isinstance(r, dict):
            return {"kind": "dict", "keys": list(r.keys())}
        return {"kind": "other", "value": str(r)[:200]}

    out = {
        "bin": args.bin,
        "query": args.query,
        "file": file_path,
        "position_1based": {"line": args.line, "col": args.col},
        "timings_ms": {
            "spawn_to_initialize_response": round((t_initialized - t_spawn) * 1000, 1),
            "spawn_to_first_query_response": round((t_first - t_spawn) * 1000, 1),
            "initialize_response_to_first_query_response": round((t_first - t_initialized) * 1000, 1),
            "spawn_to_healed_non_null_ms": healed_ms,
            "warm_query_response_ms": [round(x * 1000, 2) for x in warm_latencies],
        },
        "peak_rss_kb": rss,
        "peak_rss_mb": round(rss / 1024, 1) if rss else None,
        "raw_first_response_summary": summarize(raw_first_resp),
        "healed_response_summary": summarize(healed_resp) if healed_resp else None,
        "warm_response_summary": summarize(warm_resp) if warm_resp else None,
        "progress_notifications": progress_summary,
    }
    print(json.dumps(out, indent=2))

    if args.dump_full_response:
        with open(args.dump_full_response, "w") as f:
            json.dump((healed_resp or raw_first_resp).get("result"), f, indent=2)

    try:
        client.request("shutdown", None, timeout=10)
        client.notify("exit", None)
    except Exception:
        pass
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


if __name__ == "__main__":
    main()
