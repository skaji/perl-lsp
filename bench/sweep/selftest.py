#!/usr/bin/env python3
"""Self-test for the sweep's classification and normalisation.

The sweep's own bug class is a WRONG REPORT, which looks exactly like a
right one — a normalisation that folds two different answers together hides
a regression, and one that splits two identical answers invents a hundred.
Neither is visible by reading the output. These cases pin the rules that
decide it.

Run: python3 bench/sweep/selftest.py
"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import normalize as N
import positions as P

FAILED = []


def check(name, got, want):
    if got != want:
        FAILED.append(f"{name}\n    got  {got!r}\n    want {want!r}")


def loc(uri, l, c):
    return {"uri": uri, "range": {"start": {"line": l, "character": c},
                                  "end": {"line": l, "character": c + 3}}}


# --- normalisation ----------------------------------------------------------

# Two checkouts have different absolute roots. A URI difference is never a
# finding, and if it were not normalised EVERY position would diverge.
check("root-relative URIs compare equal",
      N.locations([loc("file:///a/root/lib/X.pm", 1, 2)], "file:///a/root"),
      N.locations([loc("file:///b/other/lib/X.pm", 1, 2)], "file:///b/other"))

# An answer that escaped the workspace must NOT normalise into looking local
# — that is exactly the kind of thing the sweep exists to catch.
check("outside-root answers stay marked",
      N.locations([loc("file:///usr/share/perl5/Foo.pm", 0, 0)], "file:///a/root")[0][0],
      "<ext>/share/perl5/Foo.pm")

# LSP lets a server answer Location, Location[] or LocationLink[] for the
# same fact. Comparing shapes instead of facts would report every position
# as divergent when one side switched encoding.
check("LocationLink folds to the same shape as Location",
      N.locations({"targetUri": "file:///r/lib/X.pm",
                   "targetSelectionRange": {"start": {"line": 1, "character": 2},
                                            "end": {"line": 1, "character": 5}}},
                  "file:///r"),
      N.locations([loc("file:///r/lib/X.pm", 1, 2)], "file:///r"))

# Order is not semantic for locations...
check("location order is not a divergence",
      N.locations([loc("file:///r/a.pm", 1, 1), loc("file:///r/b.pm", 2, 2)], "file:///r"),
      N.locations([loc("file:///r/b.pm", 2, 2), loc("file:///r/a.pm", 1, 1)], "file:///r"))

# ...but it IS for completion, where order is the ranking the user sees.
a = N.completion({"items": [{"label": "x", "kind": 3}, {"label": "y", "kind": 3}]})
b = N.completion({"items": [{"label": "y", "kind": 3}, {"label": "x", "kind": 3}]})
check("completion set is order-insensitive", a["labels"], b["labels"])
check("completion ranking is order-SENSITIVE", a["top"] != b["top"], True)

# A label that changed KIND is a different answer (a sub became a field).
check("completion kind participates in identity",
      N.completion({"items": [{"label": "x", "kind": 3}]})["labels"]
      != N.completion({"items": [{"label": "x", "kind": 5}]})["labels"], True)

# Hover carries absolute paths inside its markdown body, so the
# environmental rule has to reach inside the text too.
h1 = N.hover({"contents": {"value": "sub f — /home/alice/proj/lib/A.pm"}}, "file:///x")
h2 = N.hover({"contents": {"value": "sub f — /var/lib/other/deep/A.pm"}}, "file:///x")
check("hover paths normalise out", h1["sha"], h2["sha"])

# documentSymbol: a symbol that MOVED in the hierarchy diverges, a
# reordering of siblings does not.
tree = lambda kids: [{"name": "Pkg", "kind": 2, "children": kids}]
k1, k2 = {"name": "a", "kind": 6}, {"name": "b", "kind": 6}
check("sibling order is not a divergence",
      N.symbols(tree([k1, k2]), ""), N.symbols(tree([k2, k1]), ""))
check("a moved symbol IS a divergence",
      N.symbols(tree([k1]), "") != N.symbols([k1], ""), True)


# --- classification ---------------------------------------------------------

sys.argv = ["sweep"]
import sweep as S

def cls(verb, base, head):
    return S.classify(verb, {"norm": base}, {"norm": head})

L = lambda *n: [["lib/A.pm", [i, 0, i, 1]] for i in n]

check("identical answers are `same`", cls("definition", L(1), L(1)), "same")
check("base answered, head did not", cls("definition", L(1), []), "only-base")
check("head answered, base did not", cls("definition", [], L(1)), "only-head")
check("head found strictly more", cls("definition", L(1), L(1, 2)), "superset")
check("head found strictly fewer", cls("definition", L(1, 2), L(1)), "subset")
check("neither contains the other", cls("definition", L(1, 2), L(2, 3)), "disagree")

# A timeout is NOT an empty answer. Folding them makes a slower side look
# like it lost resolutions — the single most misleading thing this can say.
check("a head timeout is not a lost answer",
      S.classify("definition", {"norm": L(1)}, {"norm": None, "timeout": True}),
      "timeout-head")
check("a base timeout is its own shape",
      S.classify("definition", {"norm": None, "timeout": True}, {"norm": L(1)}),
      "timeout-base")

# Same candidates, different order: a ranking regression must not hide
# behind an unchanged candidate set.
r1 = {"n": 2, "labels": [["x", 3], ["y", 3]], "top": [["x", 3], ["y", 3]]}
r2 = {"n": 2, "labels": [["x", 3], ["y", 3]], "top": [["y", 3], ["x", 3]]}
check("a pure re-rank is reported, not swallowed", cls("completion", r1, r2), "reranked")

# An unimplemented verb is a capability gap, and classify must not dress it
# up as a lost answer — the diff subtracts it before counting.
check("a one-sided protocol error is its own shape",
      S.classify("definition", {"norm": None, "err": "Method not found"}, {"norm": L(1)}),
      "error-base")


# A truncated list is a strict subset of an untruncated one BY DESIGN. This
# is the rule that turned 221 "regression candidates" into one intended
# change on the first corpus run.
cap_head = {"n": 1, "labels": [["x", 3]], "top": [["x", 3]], "incomplete": True}
big_base = {"n": 2, "labels": [["x", 3], ["y", 3]], "top": [["x", 3]], "incomplete": False}
check("a truncated head list is capped, not a lost candidate",
      cls("completion", big_base, cap_head), "capped-head")
# ...but a smaller list that does NOT claim truncation is still a real subset.
uncapped_head = dict(cap_head, incomplete=False)
check("an untruncated smaller list is still a subset",
      cls("completion", big_base, uncapped_head), "subset")


# The recheck pass must SUPERSEDE the cold answer, or the sweep measures
# startup rather than the branch. Written as an appended row, applied on load.
import json as _json, tempfile as _tf
_p = _tf.NamedTemporaryFile("w", suffix=".jsonl", delete=False)
_p.write(_json.dumps({"_meta": {"side": "t"}}) + "\n")
_p.write(_json.dumps({"file": "a.pm", "line": 1, "char": 2, "verb": "definition",
                      "norm": [], "ms": 1}) + "\n")
_p.write(_json.dumps({"_recheck": True, "file": "a.pm", "line": 1, "char": 2,
                      "verb": "definition", "norm": [["lib/X.pm", [0, 0, 0, 1]]],
                      "ms": 1}) + "\n")
_p.close()
_meta, _rows = S._load(_p.name)
_row = _rows[("a.pm", 1, 2, "definition")]
check("a warm recheck supersedes the cold empty answer",
      (_row["norm"], _row.get("filled_when_warm")),
      ([["lib/X.pm", [0, 0, 0, 1]]], True))
os.unlink(_p.name)


# The noise floor is a per-answer RATE, so it is only subtractable from a
# count over the same answers. On Koha the base wedged and produced 1,184
# comparable answers where the two noise runs produced ~21,790; a floor from
# the larger population quoted beside the smaller count is not a correction,
# it is a different measurement. `_shape_counts` must intersect.
def _tmp_answers(rows):
    f = _tf.NamedTemporaryFile("w", suffix=".jsonl", delete=False)
    f.write(_json.dumps({"_meta": {"side": "t"}}) + "\n")
    for r in rows:
        f.write(_json.dumps(r) + "\n")
    f.close()
    return f.name

def _row(line, norm, **kw):
    return dict({"file": "a.pm", "line": line, "char": 0, "verb": "definition",
                 "norm": norm, "ms": 1}, **kw)

_D = [["lib/X.pm", [0, 0, 0, 1]]]
# Two noise runs that disagree at line 1 AND at line 9. Only line 1 is in the
# A/B's comparable set, so only line 1 may count toward the floor.
_n1 = _tmp_answers([_row(1, _D), _row(9, _D)])
_n2 = _tmp_answers([_row(1, []), _row(9, [])])
_counts, _cov = S._shape_counts(_n1, _n2, {"definition"},
                                {("a.pm", 1, 0, "definition")})
# Shape-level keys are plain strings; per-verb keys are (shape, verb) tuples.
# Summing indiscriminately double-counts, which is why this asserts each.
_shape_only = {k: v for k, v in _counts.items() if isinstance(k, str)}
check("the floor counts only answers the A/B actually compared",
      (sum(_shape_only.values()), _cov), (1, 1))
check("the floor is also sliced per verb, the only baseline a single-verb "
      "block can be read against",
      _counts.get(("only-base", "definition")), 1)
for _f in (_n1, _n2):
    os.unlink(_f)


# A short side is a side that STOPPED, not a side that found less. Every
# ratio over it is a ratio over the positions it got through — the cheap ones.
_short = _tmp_answers([_row(1, _D)])
_L = open(_short).read().split("\n")
_m = _json.loads(_L[0]); _m["_meta"]["expected_positions"] = 10
_L[0] = _json.dumps(_m); open(_short, "w").write("\n".join(_L))
_meta_s, _rows_s = S._load(_short)
check("a truncated side is detected against its own declared count",
      S._completeness(_meta_s, _rows_s)[:2], (1, 10))

# ...and a side that answered everything but never wrote its completion
# marker died somewhere after the main loop, which is equally not-finished.
_nomark = _tmp_answers([_row(1, _D)])
_L = open(_nomark).read().split("\n")
_m = _json.loads(_L[0]); _m["_meta"]["expected_positions"] = 1
_L[0] = _json.dumps(_m); open(_nomark, "w").write("\n".join(_L))
_meta_n, _rows_n = S._load(_nomark)
check("a run with no completion marker is not treated as complete",
      S._completeness(_meta_n, _rows_n)[2], False)

_full = _tmp_answers([_row(1, _D), {"_event": "complete", "positions_answered": 1}])
_L = open(_full).read().split("\n")
_m = _json.loads(_L[0]); _m["_meta"]["expected_positions"] = 1
_L[0] = _json.dumps(_m); open(_full, "w").write("\n".join(_L))
_meta_f, _rows_f = S._load(_full)
check("a complete run reports complete",
      S._completeness(_meta_f, _rows_f), (1, 1, True))
for _f in (_short, _nomark, _full):
    os.unlink(_f)


# --- position selection -----------------------------------------------------

check("selection is deterministic across processes — hash() is salted, "
      "and a salted sample would give the two sides different questions",
      P.stable_frac("seed", "f.pm", 1, 2, "k"),
      P.stable_frac("seed", "f.pm", 1, 2, "k"))

src = "package P;\n=pod\nsub in_pod {}\n=cut\nsub real {}\n# sub in_comment {}\n"
kinds = {(t["kind"], t["name"]) for t in P.tokens_in(src)}
check("POD bodies are not sampled", ("sub-decl", "in_pod") in kinds, False)
check("comments are not sampled", ("sub-decl", "in_comment") in kinds, False)
check("real declarations are sampled", ("sub-decl", "real") in kinds, True)

# Blanking must preserve coordinates: a sweep that shifted positions would
# ask the two sides the same question at two different places.
lines = P.strip_noncode("=pod\nxx\n=cut\nsub real {}\n").split("\n")
check("stripping preserves line numbering", lines[3], "sub real {}")

if FAILED:
    print(f"FAIL ({len(FAILED)})")
    for f in FAILED:
        print("  " + f)
    sys.exit(1)
print("sweep selftest: all checks pass")
