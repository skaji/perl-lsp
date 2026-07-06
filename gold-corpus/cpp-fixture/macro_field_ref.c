/* A member-block macro (BASEOP-style) replicated across a struct FAMILY, and a
 * `->op_next` field drill buried in ANOTHER macro's body (LINKLIST-style).
 * The body is one opaque preproc_arg, so the field use has no query capture:
 * the macro-body member lane recovers it and references on `op_next` include
 * the in-body use. Family D (hitlist-6). */
#define BASEOP  struct op* op_next; int op_type;
struct op   { BASEOP };
struct unop { BASEOP struct op* op_first; };
#define LINKLIST(o)  ((o)->op_next ? (o)->op_next : 0)
int walk(struct op* o) {
    o->op_next = 0;
    return LINKLIST(o) ? o->op_type : 0;
}
