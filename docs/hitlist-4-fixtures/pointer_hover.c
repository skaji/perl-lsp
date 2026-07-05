/* Control for hitlist-4 finding 4: hover on op_next here shows `op_next: OP*`
   (pointer kept). In perl5, where the field is declared inside `#define BASEOP`,
   the same hover shows `op_next: OP` — extraction loss on the macro-body lane. */
#include "op_mini.h"
void use_l(OP* o) {
    o->op_next;
}
