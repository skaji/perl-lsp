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
