#!/usr/bin/env python3
"""Position selection for the differential sweep.

The sample must be identical for both sides, so it is derived from the
SOURCE TEXT alone — never from either binary. A binary-derived sample (say,
semantic tokens) would let one side choose where it is tested, and a
divergence would then be unattributable: did the answer change, or did the
question?

Selection is token-driven because random offsets land on whitespace. Each
emitted position carries the token KIND it came from, which is what makes
the divergence report groupable ("47 method-call positions where the branch
resolves and the base does not" is reviewable; 47 hunks are not).

The tokenizer is deliberately CONSERVATIVE, and every heuristic here fails
toward emitting FEWER positions rather than positions in places a Perl
reader would not call a token. Under-sampling costs coverage, which is
visible in the totals; over-sampling costs the report's signal, which is
not. String interiors are the known residual — a token inside `"..."` can
still be emitted. That is handled downstream instead of here: the diff drops
any position where both sides answered nothing on every verb, which removes
uninformative samples generically rather than by perfecting a regex.
"""
import hashlib, re

# `sub` / `method` are both declaration keywords in modern Perl.
RE_SUB      = re.compile(r'\b(?:sub|method)\s+([A-Za-z_]\w*)')
RE_PACKAGE  = re.compile(r'\b(?:package|class|role)\s+([A-Za-z_][\w:]*)')
RE_USE      = re.compile(r'\b(?:use|no|require|extends|with)\s+([A-Za-z_][\w:]*)')
RE_METHOD   = re.compile(r'->\s*([A-Za-z_]\w*)')
RE_CALL     = re.compile(r'\b([A-Za-z_]\w*)\s*\(')
RE_VAR      = re.compile(r'[\$\@\%]([A-Za-z_]\w*)')
RE_MODPATH  = re.compile(r'\b([A-Za-z_]\w*(?:::\w+)+)\b')
RE_FATKEY   = re.compile(r'\b([A-Za-z_]\w*)\s*=>')
RE_HASHKEY  = re.compile(r'\{\s*[\'"]?([A-Za-z_]\w*)[\'"]?\s*\}')

# Perl builtins and flow keywords that `NAME(` would otherwise sample as
# call sites. They are not interesting differentially — every side agrees —
# and they are numerous enough to crowd out real call sites.
KEYWORDS = set("""
if elsif unless while until for foreach do sub my our local return last next redo
and or not eq ne lt gt le ge cmp x qw qq q tr y s m defined ref bless wantarray
print printf sprintf push pop shift unshift splice join split map grep sort reverse
keys values each exists delete scalar length substr index rindex uc lc ucfirst lcfirst
die warn eval require use no package BEGIN END chomp chop sprintf open close
""".split())

PATTERNS = [
    ("sub-decl",    RE_SUB),
    ("package",     RE_PACKAGE),
    ("use-module",  RE_USE),
    ("method-call", RE_METHOD),
    ("module-path", RE_MODPATH),
    ("call-site",   RE_CALL),
    ("hash-key",    RE_FATKEY),
    ("hash-key",    RE_HASHKEY),
    ("variable",    RE_VAR),
]


def strip_noncode(text):
    """Blank out POD, `__END__`, and comments, preserving line/column numbers.

    Replacement is space-for-character rather than deletion so every
    surviving token keeps its true LSP coordinates — a sweep that shifted
    positions would compare two sides at the same index into different text.
    """
    out, in_pod = [], False
    for line in text.split("\n"):
        if line.startswith("__END__") or line.startswith("__DATA__"):
            out.extend([""] * (len(text.split("\n")) - len(out)))
            break
        if re.match(r'^=[a-zA-Z]', line):
            in_pod = True
        if in_pod:
            out.append(" " * len(line))
            if line.startswith("=cut"):
                in_pod = False
            continue
        # A `#` that starts a comment, as opposed to `$#array`, `${#}`, or a
        # `#` inside a string. Only the first two are distinguishable cheaply,
        # so this truncates on the first `#` not preceded by `$`. Inside a
        # string that loses the rest of the line: under-sampling, by design.
        m = re.search(r'(?<!\$)#', line)
        if m:
            line = line[:m.start()] + " " * (len(line) - m.start())
        out.append(line)
    return "\n".join(out)


def tokens_in(text):
    """Every candidate position in one file, deduped by (line, col, kind).

    Patterns overlap on purpose — `Foo::Bar->new` is both a module path and a
    method call, and both are worth asking about. Dedup is on the triple, so
    the same offset can appear under two kinds but never twice under one.
    """
    seen, out = set(), []
    for lineno, line in enumerate(strip_noncode(text).split("\n")):
        for kind, rx in PATTERNS:
            for m in rx.finditer(line):
                name = m.group(1)
                if kind == "call-site" and name in KEYWORDS:
                    continue
                if len(name) < 2:
                    continue
                col = m.start(1)
                key = (lineno, col, kind)
                if key in seen:
                    continue
                seen.add(key)
                out.append({
                    "line": lineno, "char_on": col,
                    "char_after": col + len(name),
                    "kind": kind, "name": name,
                })
    return out


def stable_frac(*parts):
    """A deterministic [0,1) from the sample's identity.

    `hash()` is salted per process in Python 3, so a run would sample a
    different subset every time and two sides could not be compared at all.
    """
    h = hashlib.sha256("\x00".join(str(p) for p in parts).encode()).digest()
    return int.from_bytes(h[:8], "big") / 2**64


# Per-kind quotas. An unweighted sample is ~71% `variable` on real CPAN code
# (measured on CGI.pm: 3,160 of 4,422), and a scalar in a lexical scope is the
# case both sides are most likely to agree on — so an unweighted sweep spends
# most of its budget confirming a null result. The quotas buy signal density,
# and they are a SAMPLE shape, not a claim about importance: `variable` stays
# represented because variable resolution is exactly where a scope or
# visibility change would show up.
KIND_WEIGHT = {
    "method-call": 1.00,
    "call-site":   1.00,
    "sub-decl":    1.00,
    "module-path": 1.00,
    "use-module":  1.00,
    "package":     1.00,
    "hash-key":    0.80,
    "variable":    0.15,
}


def select(rel_path, text, per_file_cap, seed):
    """The sampled positions for one file, deterministic in (seed, path).

    Capped per file so one 20k-line module cannot crowd out a hundred small
    ones — a sweep whose sample is dominated by its largest file measures
    that file, not the corpus.
    """
    toks = tokens_in(text)
    keep = []
    for t in toks:
        w = KIND_WEIGHT.get(t["kind"], 0.5)
        if stable_frac(seed, rel_path, t["line"], t["char_on"], t["kind"]) < w:
            keep.append(t)
    if len(keep) > per_file_cap:
        # Round-robin ACROSS kinds, not a uniform draw over the pool. A
        # uniform cap re-imposes the raw distribution the weights just
        # corrected — measured on CGI.pm, a flat cap of 40 came back 17
        # `variable` and 2 `method-call`, undoing the whole point. Taking one
        # kind at a time guarantees the rare-but-interesting kinds survive a
        # file whose token count dwarfs the cap.
        by_kind = {}
        for t in keep:
            by_kind.setdefault(t["kind"], []).append(t)
        for k, lst in by_kind.items():
            lst.sort(key=lambda t: stable_frac(seed, "cap", rel_path, t["line"], t["char_on"], k))
        keep, order = [], sorted(by_kind)
        i = 0
        while len(keep) < per_file_cap and any(by_kind.values()):
            lst = by_kind[order[i % len(order)]]
            if lst:
                keep.append(lst.pop(0))
            i += 1
    keep.sort(key=lambda t: (t["line"], t["char_on"], t["kind"]))
    for t in keep:
        t["file"] = rel_path
    return keep
