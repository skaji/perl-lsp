#include "optype.h"
#include "regmacro.h"

void prune(OP** op_p) {
    OP* node = *op_p;
    if (OP(node) == 0) {
        node = node->op_next;
    }
}
