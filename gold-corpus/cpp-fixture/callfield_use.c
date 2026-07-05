#include "callfield.h"

int cf_total(void) {
    CfGadget *g = cfMakeGadget();
    int x = g->weight;
    int y = cfMakeGadget()->height;
    return x + y;
}
