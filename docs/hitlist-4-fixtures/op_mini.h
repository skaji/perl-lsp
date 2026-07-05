/* Reduced perl5 op.h: plain-struct CONTROL for the BASEOP macro-body lane.
   Fields declared directly in the struct keep deref_stack (hover `op_next: OP*`)
   and their ANNOT witness suppresses the inlay hint — the perl5 BASEOP lane
   loses both (hitlist-4 family C). */
typedef struct op OP;
typedef enum opcode { OP_NULL = 0, OP_STUB = 1, OP_SCALAR = 2 } opcode;
struct op {
    OP* op_next;
    unsigned op_type:9;
};
