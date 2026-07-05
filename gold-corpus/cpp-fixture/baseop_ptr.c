/* hitlist-4 family C: an object-like member-block macro (BASEOP) whose body
   declares a POINTER member. Hover on the op_next def-site must keep the `*`
   (OP*), exactly like a plainly-declared struct field — the macro-body member
   lane no longer drops the deref stack. The `o->op_next` use exercises the
   member-access lane over the same synthesized field. */
typedef struct op OP;
#define BASEOP OP* op_next; unsigned op_type:9;
struct op { BASEOP };
void use_op(OP* o) {
    o->op_next;
}
