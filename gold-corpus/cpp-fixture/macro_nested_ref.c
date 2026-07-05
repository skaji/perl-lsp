/* A macro whose BODY references another macro. References on the inner macro
 * should include the use inside the outer macro's `#define` body — but macro
 * definition bodies are preproc-excluded from ref minting, so the nested use is
 * unindexed (perl5 `SvFLAGS` used inside `SvOK`/`SvTRUE` etc: 190/347 real).
 * Family M #3 — PARKED, references-index-population only (goto-def works). */
#define FLAGS(x)  (x)->f
#define IS_OK(x)  (FLAGS(x) & 1)

struct s { int f; };

int t(struct s* p) {
    int a = FLAGS(p);
    return IS_OK(p);
}
