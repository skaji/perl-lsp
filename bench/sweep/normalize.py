#!/usr/bin/env python3
"""What counts as "the same answer".

A sweep whose diff is mostly noise is worse than no sweep, so every rule
here is a deliberate decision about which differences are semantic. They
fall into three groups:

  ENVIRONMENTAL — absolute paths, and the workspace root itself. Two sides
  run from two checkouts; a URI difference is never a finding.

  ORDER — LSP arrays are mostly unordered sets (definition locations,
  references, document symbols). Comparing them as lists reports a
  divergence for every stable-sort difference. Completion is the exception
  and is treated as one: its order IS the ranking, which is a user-visible
  answer, so it is compared BOTH ways — as a label set (did the candidates
  change) and as an ordered prefix (did the ranking change). Conflating
  those two hides a ranking regression behind an unchanged set.

  VOLUME — hover bodies and completion documentation run to kilobytes and
  are usually identical. They are digested, not dropped: a digest still
  diverges when the text changes, but the report stays readable. Dropping
  them would make a documentation regression invisible; keeping them would
  make the report unreadable.
"""
import hashlib, json, re


def digest(s):
    return hashlib.sha256(s.encode("utf8", "replace")).hexdigest()[:12]


def rel_uri(uri, root_uri):
    """`file:///abs/root/lib/Foo.pm` -> `lib/Foo.pm`.

    Paths outside the root keep a marked absolute form rather than being
    silently relativised: an answer that escaped the workspace is exactly
    the kind of thing this sweep exists to catch, and it must not be
    normalised into looking local.
    """
    if not isinstance(uri, str):
        return uri
    if root_uri and uri.startswith(root_uri):
        return uri[len(root_uri):].lstrip("/")
    # @INC providers live outside the root and their absolute prefix differs
    # per machine, not per branch. Keep the tail, mark it foreign.
    return "<ext>/" + "/".join(uri.rsplit("/", 3)[-3:])


def rng(r):
    if not isinstance(r, dict):
        return None
    s, e = r.get("start") or {}, r.get("end") or {}
    return [s.get("line"), s.get("character"), e.get("line"), e.get("character")]


def locations(result, root_uri):
    """definition / typeDefinition / references / implementations.

    LSP allows Location, Location[], LocationLink[], or null for these, and
    the three shapes carry the same fact. Folding them to one shape is what
    lets a side that answers LocationLink compare against a side that
    answers Location -- otherwise every position would diverge on shape
    alone and the sweep would report nothing but its own encoding.
    """
    if result is None:
        return []
    items = result if isinstance(result, list) else [result]
    out = []
    for it in items:
        if not isinstance(it, dict):
            continue
        uri = it.get("uri") or it.get("targetUri")
        span = it.get("range") or it.get("targetSelectionRange") or it.get("targetRange")
        out.append((rel_uri(uri, root_uri), tuple(rng(span) or [])))
    return sorted(set(out))


def hover(result, root_uri):
    if not result:
        return None
    c = result.get("contents")
    if isinstance(c, dict):
        text = c.get("value", "")
    elif isinstance(c, list):
        text = "\n".join(x.get("value", x) if isinstance(x, dict) else str(x) for x in c)
    else:
        text = str(c or "")
    # Absolute paths appear inside hover markdown ("defined in /abs/..."), so
    # the ENVIRONMENTAL rule has to reach inside the body too.
    text = re.sub(r'(/[\w.\-]+){3,}', '<path>', text).strip()
    text = re.sub(r'\s+', ' ', text)
    return {"len": len(text), "sha": digest(text), "head": text[:160]}


COMPLETION_TOP_N = 10


def completion(result):
    """`isIncomplete` is kept because it is the ONLY thing that distinguishes
    a lost candidate from a deliberately cut one.

    A server that ranks and truncates (perl-lsp caps at
    `MAX_COMPLETION_ITEMS`) returns a strict subset of an uncapped server's
    list and sets `isIncomplete` to say so — that is the LSP contract, not a
    defect. Dropping the flag made the first corpus run report 221 `subset`
    rows as regression candidates when they were one intended change.
    """
    if result is None:
        return {"n": 0, "labels": [], "top": [], "incomplete": False}
    inc = bool(result.get("isIncomplete")) if isinstance(result, dict) else False
    items = result.get("items", []) if isinstance(result, dict) else result
    if not isinstance(items, list):
        return {"n": 0, "labels": [], "top": [], "incomplete": inc}
    # (label, kind): the same label at a different kind is a different
    # answer -- a sub becoming a field is a real change, and comparing bare
    # labels would hide it.
    pairs = [(str(i.get("label", "")), i.get("kind")) for i in items if isinstance(i, dict)]
    return {
        "n": len(pairs),
        "labels": sorted(set(pairs)),
        "top": pairs[:COMPLETION_TOP_N],
        "incomplete": inc,
    }


def symbols(result, root_uri):
    """documentSymbol, flattened to a nesting-aware set.

    The path (`Foo::bar`) is carried into the key so a symbol that MOVED in
    the hierarchy diverges, while a reordering of siblings does not.
    """
    out = []

    def walk(items, prefix):
        for it in items or []:
            if not isinstance(it, dict):
                continue
            name = it.get("name", "")
            key = f"{prefix}::{name}" if prefix else name
            out.append((key, it.get("kind")))
            walk(it.get("children"), key)

    walk(result if isinstance(result, list) else [], "")
    return sorted(set(out))


def normalize(verb, result, root_uri):
    if verb in ("definition", "typeDefinition", "references", "implementations"):
        return locations(result, root_uri)
    if verb == "hover":
        return hover(result, root_uri)
    if verb == "completion":
        return completion(result)
    if verb == "documentSymbol":
        return symbols(result, root_uri)
    return result


def answer_key(verb, norm):
    """A stable string for equality, so the diff never compares floats or
    dict orderings."""
    return json.dumps(norm, sort_keys=True, separators=(",", ":"), default=str)


def is_empty(verb, norm):
    if norm is None:
        return True
    if verb == "completion":
        return norm.get("n", 0) == 0
    if verb == "hover":
        return not norm or norm.get("len", 0) == 0
    if isinstance(norm, (list, tuple)):
        return len(norm) == 0
    return False


def _froze(x):
    """Deep tuple-ify. A JSON round-trip turns every tuple into a list, so an
    answer read back from the run file is `["lib/A.pm", [1, 2, 1, 5]]` — and
    building a set of those raises `unhashable type: 'list'` on the nested
    span, not on the outer element."""
    if isinstance(x, (list, tuple)):
        return tuple(_froze(i) for i in x)
    return x


def as_set(verb, norm):
    """The comparable SET behind an answer, or None if the verb has none.

    Set containment is what turns a divergence into a claim someone can
    adjudicate: "the branch found everything the base did, plus 3" reviews
    differently from "the two disagree".
    """
    if norm is None:
        return set()
    if verb in ("definition", "typeDefinition", "references", "implementations"):
        return set(_froze(x) for x in norm)
    if verb == "completion":
        return set(_froze(x) for x in norm.get("labels", []))
    if verb == "documentSymbol":
        return set(_froze(x) for x in norm)
    return None
