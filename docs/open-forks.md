# Open architectural forks — for discussion

Convention (standing order, 2026-07-03): when autonomous work hits a genuine
architectural fork, we (a) pick the LOOSELY-COUPLED option — reversible,
behind a seam, no serialized-format lock-in where avoidable — (b) implement
it, and (c) log the fork here with the options, what was picked, why, and
what undoing it would cost. The user reviews this ledger; entries get
resolved (ratified or reversed) explicitly.

This file holds ONLY the open forks. Resolved/ratified/closed entries move
to `docs/forks-resolved.md` (ledger of record); deferred work items with
designs live in `docs/prompt-storage-residuals.md`.

**Awaiting review:**

| Fork | Since | The question |
| --- | --- | --- |
| [Invocant class vs inner-scope rep narrowing](#invocant-class-vs-inner-scope-rep-narrowing--2026-07-15--open) | 07-15 | is the residual ctor-gap (`my $self = $class->new`) worth cross-file receiver-polymorphic `new`, or option A's blanket `$self` assertion? |
| [Answer honesty under index/enrichment windows](#answer-honesty-under-indexenrichment-windows--2026-07-14--open-claude) | 07-14 | which verbs block for honest answers vs stay fast-best-effort — now that honest cold references costs ~27 s on abseil? |
| [Pack first-change diagnostics](#pack-first-change-diagnostics-fast-degraded-now-vs-correct-but-delayed--2026-07-15--open) | 07-15 | is stale-but-fast the right default for the first change after a cold open? |
| [Decl→def ranking on QUALIFIED / member goto-def](#decldef-ranking-on-qualified--member-goto-def--2026-07-15--open-claude) | 07-15 | should qualified goto-def rank def-over-decl, via the shared seam (B) or a local patch (A)? |

Format per entry:

## <fork name> — <date> — <status: OPEN / ratified / reversed>
- **Context:** where it came up (slice, finding).
- **Options:** A / B (/ C), one line each.
- **Picked:** which, and the loose-coupling story (how it stays undoable).
- **Undo cost:** what reversing takes.
- **Discussion needed:** the question for the user.

---

## Invocant class vs inner-scope rep narrowing — 2026-07-15 — OPEN
- **Context:** edit-bench finding (`bench/RESULTS.md`): `$self->` completion
  returned a lone `{key} hash dereference` inside `use base` classes
  (Bugzilla::Bug) instead of the class's methods. Root cause was NOT that
  the invocant fails to type — a conventional invocant already types as
  `ClassName`/`FirstParam` (via `detect_first_param_type`, independent of
  framework). The bug: `$self->{field}` accesses inside a nested block push
  rep witnesses (`HashRefAccess` observation + an `infer_deref_type`
  `HashRef` TC) on the *inner* scope, while the invocant's class lives on the
  sub scope. The scope-chain walk (`query_variable_with_visited`) returned
  the innermost typed scope first → `HashRef`, masking the class.
- **Options:** A — always assert every conventional invocant scalar
  (`$self`/`$class`) as `ClassName(current_package)` via a witness,
  matching `invocant_type_at_node`'s build-time `$self` short-circuit
  (aggressive; also masks a genuinely hashref-typed `$self` and needs a
  sub-Builder confidence tier to lose to real bindings). B — framework-gate
  the rep suppression (wrong — the bug is framework-independent). C —
  identity-over-rep across the scope walk: class identity anywhere in the
  chain dominates an inner scope whose answer is a *rep-observation-only*
  projection, and `subsumes_narrowing` stops `infer_deref_type` re-deriving
  a rep `HashRef` over an existing class identity at the access site.
- **Picked:** C. Two small, reversible seams: (1) `subsumes_narrowing`
  gains one arm — a class-identity `self` subsumes a `HashRef`/`ArrayRef`/
  `CodeRef` narrowing (lifts the existing structure-over-rep rule to
  class-over-rep); (2) `query_variable_with_visited` defers a scope answer
  that isn't a class identity AND whose scope only OBSERVES rep use
  (`scope_binds_variable` gate) to an outer class identity. Fixes the
  invocant case at its true root without asserting anything about `$self`
  that the code didn't already establish; genuine inner-scope rebindings
  (`my $h = {…}`) stay authoritative because they BIND the variable.
- **Undo cost:** small — both seams are localized (one match arm in
  `file_analysis.rs::subsumes_narrowing`, one helper + walk tweak in
  `witnesses.rs`). Removing them restores the innermost-scope-wins walk.
- **Discussion needed:** the residual `check`-style case — a LOCAL
  `my $self = $class->new(...)` where the cross-file base ctor
  (`Bugzilla::Object::new`) resolves to `None` — still leaves `$self`
  untyped→`HashRef` (2 of 60 real `$self->` sites in Bug.pm). That's a
  cross-file constructor-return gap, not invocant typing; fixing it means
  either resolving receiver-polymorphic `new` cross-file, or adopting
  option A's aggressive "every `$self` is the enclosing class" assertion.
  Worth it?

## Answer honesty under index/enrichment windows — 2026-07-14 — OPEN (Claude)
- **Context:** edit-bench rounds 1–4 (bench/RESULTS.md). Verbs answer
  PARTIAL or NULL inside two windows and the response looks complete:
  cold index build (curl cold references 866 B vs 34 KB warm; bugzilla
  cold completion 233 B vs 5.5 KB) and per-file build/enrichment waits
  (bugzilla WARM outline sometimes null, WARM hover sometimes null —
  the ~400 ms bounded waits `await_open_ready`/`await_index_ready`
  expire and the verb serves whatever is there). Editor-tier sibling of
  absence-as-answer.
- **Options:** A — per-verb wait policy on one seam: bulk/identity verbs
  (references, rename, implementations) wait for index-ready without the
  400 ms cap (with LSP progress); per-file verbs (outline, hover,
  completion) wait for THIS file's build (bounded by build time, not a
  fixed cap); latency-critical interactive verbs keep best-effort.
  B — always best-effort + server-initiated refresh nudges (works for
  semanticTokens/inlayHint; LSP has NO refresh channel for
  references/hover/outline responses — can't heal those).
  C — label partial answers (LSP has no partiality flag on these verbs;
  would need client cooperation).
- **Picked (to implement):** A — it's the only shape that can't lie on
  verbs whose answers are act-on-able (rename edits!), and the policy
  lives on ONE seam (the existing await_* helpers grow a per-verb
  policy parameter) so redirecting any verb's policy later is a
  one-line change. B's nudge pattern stays for the verbs that have
  refresh channels.
- **Undo cost:** trivial per verb — the policy table is data.
- **Discussion needed:** which verbs the user wants blocking-honest vs
  fast-best-effort; whether rename should hard-refuse (error) instead
  of wait when the index is cold. Concrete price now measured: abseil
  COLD references blocks ~27 s for the honest answer (was 402 ms
  partial). LSP progress for blocking waits is landed
  (`Backend::bounded_wait_with_progress` — silent under 500 ms, so
  Interactive waits never mint a token), so the block is visible in the
  editor rather than reading as a hung request.
- **New evidence (2026-07-15), the curl server-context case:** server
  references answer 4 sites where the CLI answers 155 —
  warm-deterministic, predates the fixing round. Eliminated: row
  narrowing (identical off), candidate retrieval (17 candidates, same
  as CLI), rehydration (strict clean), block view (whole_present).
  Remaining suspect: the OPEN doc's cached-only build mints a weaker
  pack target than the CLI's fully-gathered staging, so the matcher
  rejects most candidates. Repro: bench curl scenario warm +
  PERL_LSP_REFS_DEBUG=1.

## Pack first-change diagnostics: fast-degraded-now vs correct-but-delayed — 2026-07-15 — OPEN
- **Context:** edit-bench P1 (bench/RESULTS.md). The first didChange on a
  cold-opened C++ file published diagnostics in ~24 s (warm 193 ms). Root
  cause: `spawn_debounced_rebuild` ran the pack analyze with the cross-file
  GATHER enabled, so the first keystroke after a cold open paid the whole
  cold gather synchronously inside the debounce task — and did_open's
  background `spawn_pack_gather_refresh` couldn't warm it because that task
  bails once the buffer text changes.
- **Options:** A — first change rebuilds CACHED-ONLY (instant, degraded
  diagnostics), then a background gather refresh heals full-quality
  diagnostics when the cold gather lands (the same async-refresh did_open
  uses). B — share the in-flight open gather via a per-URI completion token
  and have the change path await it (correct diagnostics, but the first
  change still waits ~24 s and the token/registry is new shared state).
- **Picked:** A. Loosest-coupled: reuses the existing
  `set_gather_cached_only` thread-local and `spawn_pack_doc_refresh` heal;
  no new shared state, no cross-task handshake. The change path is symmetric
  with the open path. Cost: the first change's diagnostics are DEGRADED
  (cached-only macro table) for the ~24 s until the background gather warms
  the shared `pre_expanded_cache`; every rebuild after that is fast AND
  full-quality (cache hit). One redundant cold gather can run (did_open's G0
  bails, the change's heal G1 recomputes) — bounded, warm-cache-idempotent.
- **Undo cost:** trivial to revert to B's shape — drop the cached-only
  wrap + heal spawn, add a shared token; the seam is one function.
- **Discussion needed:** is stale-but-fast the right default for pack
  first-change, or should the first change block on correct diagnostics?
  If a shared-gather token is wanted anyway (to also kill the redundant
  double gather), that's the B upgrade — additive on top of A.

## Decl→def ranking on QUALIFIED / member goto-def — 2026-07-15 — OPEN (Claude)
- **Context:** the C-tier bench finding "C goto-def stops at the header
  prototype" (bench/RESULTS.md). Fixed for UNqualified free-function calls
  (redis `lookupKeyReadOrReply`/`addReplyBulk`, curl
  `Curl_conn_cf_discard_all`): `CandidateSet::preferred_definitions` now
  admits a def-candidate whose TU includes the DECL's header, so a third TU
  calling through a shared prototype reaches the bodied definition (ranked
  first, decl kept). But the QUALIFIED / namespaced spelling
  (`pkg::Combine` in the multitu fixture) routes through
  `member_def_location` (the owner-anchored `qualifier_at_point` path at the
  top of `definitions()`), which returns a SINGLE location, applies the same
  origin-only connectivity gate (excluding the defining TU), and does NO
  decl→def ranking — so it still lands on the prototype.
- **Options:** A — teach `member_def_location` the same decl-connectivity
  clause AND a bodied-over-bodiless preference, returning the def (or def
  ranked first). B — route qualified member/namespaced-function calls
  through `preferred_definitions` (the free-function lane already fixed) so
  one mechanism serves both spellings; `member_def_location` stays the
  member-RESOLUTION seam, ranking becomes a projection concern. C — leave
  qualified member goto-def landing on the decl and expose the def via
  `textDocument/declaration` vs `definition` split.
- **Picked:** none yet — the free-function fix is landed and scoped to the
  bench finding; the qualified-member case is a strictly-additional surface
  (the bench did not flag it, no regression introduced). Documented so the
  maintainer can pick B (the loosely-coupled unification — one decl→def
  mechanism, member_def_location keeps resolving, ranking is inherited) vs A
  (local patch, faster but re-derives the ranking in a second place, the
  asymmetry the resolution-CandidateSet ADR warns against).
- **Undo cost:** low — the landed change is one added `||` clause in
  `preferred_definitions`; picking any option above is net-new work, not a
  reversal.
- **Discussion needed:** should member/qualified goto-def rank def-over-decl
  at all, and if so via the shared `preferred_definitions` seam (B) or a
  local `member_def_location` patch (A)? B is the rule-#10-consistent pick.
