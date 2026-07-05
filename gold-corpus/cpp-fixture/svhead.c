/* A function-like member-block macro whose backslash-continued body carries a
 * trailing block comment on each field line (the perl5 sv.h `_SV_HEAD` shape).
 * tree-sitter-cpp ends `preproc_arg` at the first such comment, so extracting
 * the body from the CST span kept only `sv_any`; the body must come from raw
 * source across the continuations. The final field has no `;` of its own — the
 * `;` belongs to the paste (`_SV_HEAD(void*);`). */
#define _SV_HEAD(ptrtype) \
    ptrtype  sv_any;    /* pointer to body */    \
    unsigned sv_refcnt; /* how many refs to us */ \
    unsigned sv_flags   /* what we are */

struct svrec { _SV_HEAD(void*); };

void use(struct svrec* s) {
    unsigned a = s->sv_flags;
    unsigned b = s->sv_refcnt;
    void*    c = s->sv_any;
}
