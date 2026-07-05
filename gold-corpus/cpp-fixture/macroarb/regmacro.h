/* A deliberately re-scoped regex-internal macro, mirroring perl5 regcomp.h:
   #undef OP then a function-like #define OP(p). It must NOT out-claim the
   typedef OP a parenless type token names. */
#undef OP
#define OP(p)   ((p)->type)
