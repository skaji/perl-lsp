#include "ctx.h"

/* Control: single-level context-param macro. */
char *
rcpv_copy(pCTX_ char *pv) {
    RCPV *rcpv = RCPVx(pv);
    rcpv->refcount++;
    return pv;
}

/* The perl5 shape: nested pTHX_ chain in the parameter list. */
char *
Perl_rcpv_copy(pTHX_ char *pv) {
    RCPV *rcpv = RCPVx(pv);
    rcpv->refcount++;
    return pv;
}
