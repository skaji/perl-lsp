#ifndef CTX_H
#define CTX_H
#include <stddef.h>

typedef struct interp PerlInterpreter;

/* Context-param macros, perl5 pTHX_/pTHX shape: config-superposed, one arm
   injects a leading parameter, the other injects nothing.
   Two flavors:
   - pCTX_/pCTX: single-level expansion (the control — extraction survives).
   - pTHX_/pTHX: the real perl5 chain — nested three deep with a trailing
     attribute macro. */
#define __attribute__unused__ __attribute__((unused))
#define PERL_UNUSED_DECL __attribute__unused__

#ifdef MY_IMPLICIT_CONTEXT
#  define pCTX   void *my_ctx
#  define pCTX_  void *my_ctx,
#  define tTHX   PerlInterpreter *
#  define pTHX   tTHX my_perl PERL_UNUSED_DECL
#  define pTHX_  pTHX,
#else
#  define pCTX   void
#  define pCTX_
#  define pTHX   void
#  define pTHX_
#endif

struct rcpv {
    size_t  refcount;
    char    pv[1];
};
typedef struct rcpv RCPV;
#define RCPVx(pv_arg) ((RCPV *)((pv_arg) - 8))
#endif
