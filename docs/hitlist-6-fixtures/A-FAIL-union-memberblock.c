/* Family A repro (FAIL). A struct that pastes a member-block macro whose
 * body contains an anonymous union (perl5 _SV_HEAD_UNION -> `union {..} sv_u`),
 * stacked with another member-block macro, loses its member-block parent edge.
 * The base members (sv_flags etc.) are minted but never attached to the struct,
 * so gd/hover/completion on `sv->sv_flags` all go dark.
 * REPRO: gd on sv_flags at the return below -> want _SV_HEAD decl; gets nothing.
 * Seam: src/cpp_reparse.rs::plan_member_blocks (blank/damage-gate +
 * enclosing_aggregate_name) -> src/language_driver.rs::inject_member_blocks. */
#define _SV_HEAD(ptrtype) \
    ptrtype	sv_any;		\
    unsigned	sv_refcnt;	\
    unsigned	sv_flags
#define _SV_HEAD_UNION \
    union { char* svu_pv; long svu_iv; } sv_u
typedef struct myagg SV;
struct myagg {
    _SV_HEAD(void*);
    _SV_HEAD_UNION;
};
int use(SV *sv) {
    return sv->sv_flags;
}
