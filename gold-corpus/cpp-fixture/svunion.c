/* perl5 sv.h shape: `struct STRUCT_SV` is built by STACKING two member-block
 * macros — `_SV_HEAD(void*)` (plain fields) and `_SV_HEAD_UNION` (whose body
 * is an anonymous `union { ... } sv_u`). The union-bearing member block must
 * still attach its `(struct -> macro)` parent edge, so member navigation on an
 * `SV *` receiver resolves the stacked base's fields. Comment-carrying,
 * `\`-continued bodies (the real sv.h) — the union body has nested braces. */
#define _SV_HEAD(ptrtype) \
    ptrtype	sv_any;		/* pointer to body */	\
    unsigned	sv_refcnt;	/* how many refs to us */	\
    unsigned	sv_flags	/* what we are */
#define _SV_HEAD_UNION \
    union {				\
        char*   svu_pv;			\
        long    svu_iv;			\
    }	sv_u
typedef struct STRUCT_SV SV;
struct STRUCT_SV {
    _SV_HEAD(void*);
    _SV_HEAD_UNION;
};
unsigned sv_flags_of(SV *sv) {
    return sv->sv_flags;
}
