/* Case B repro: config-twin regions (perl5 op.c PERL_DEBUG_READONLY_OPS
   shape). A member access `o->slabbed` resolves at an unconditional site
   and goes dark at a site inside `#ifdef ... #endif`. This isolates the
   variables: pTHX_ vs plain param, #ifdef vs #if/#else vs #ifndef, and a
   struct field declared inside a conditional region.

   Verdict (spike, docs/adr/config-superposition-declarations.md): the
   darkness is NOT the #ifdef region — it is the macro-expansion exclusion
   (cpp_reparse EXCLUDE_QUERY) skipping the region BODY, so a `pTHX_` use
   inside `#ifdef` never expands and mis-parses the parameter list. Plain
   params inside `#ifdef` resolve fine; only pTHX_ + conditional breaks. */

/* perl5 pTHX_ chain, condensed from ctxparam. */
typedef struct interp PerlInterpreter;
#define PERL_UNUSED_DECL __attribute__((unused))
#ifdef MY_IMPLICIT_CONTEXT
#  define tTHX  PerlInterpreter *
#  define pTHX  tTHX my_perl PERL_UNUSED_DECL
#  define pTHX_ pTHX,
#else
#  define pTHX  void
#  define pTHX_
#endif

struct op {
    struct op *op_next;
    unsigned   slabbed;
};

/* 1. CONTROL: unconditional, plain param. Expect: slabbed resolves. */
void plain_uncond(struct op *o) {
    o->slabbed = 1;
}

/* 2. CONTROL: unconditional, pTHX_ param. Expect: slabbed resolves. */
void thx_uncond(pTHX_ struct op *o) {
    o->slabbed = 1;
}

/* 3. plain param, whole fn inside #ifdef ... #endif (single arm, no #else). */
#ifdef PERL_DEBUG_READONLY_OPS
void plain_ifdef(struct op *o) {
    o->slabbed = 1;
}
#endif

/* 4. pTHX_ param, whole fn inside #ifdef ... #endif (the real op.c shape). */
#ifdef PERL_DEBUG_READONLY_OPS
void thx_ifdef(pTHX_ struct op *o) {
    o->slabbed = 1;
}
#endif

/* 5. #if/#else twin: same fn name, two arms. Probe BOTH arms. */
#if defined(PERL_DEBUG_READONLY_OPS)
void twin(struct op *o) {
    o->slabbed = 1;         /* first (#if) arm */
}
#else
void twin(struct op *o) {
    o->slabbed = 2;         /* #else arm */
}
#endif

/* 6. struct field itself inside #ifdef — does the field def survive, and
   do refs to it resolve? */
struct thing {
    int always;
#ifdef PERL_DEBUG_READONLY_OPS
    int cond_field;
#endif
};

void uses_condfield(struct thing *t) {
    t->cond_field = 1;
    t->always = 1;
}

/* 7. #if/#else twin WITH a pTHX_ use in each arm — does the mechanism differ
   between the (inactive) #if arm and the (active) #else arm? */
#if defined(PERL_DEBUG_READONLY_OPS)
void twin_thx(pTHX_ struct op *o) {
    o->slabbed = 1;         /* #if arm (config-inactive) */
}
#else
void twin_thx(pTHX_ struct op *o) {
    o->slabbed = 2;         /* #else arm (config-active) */
}
#endif

/* 8. #ifndef spelling (always-active in default config) with a pTHX_ use. */
#ifndef SOMETHING_NEVER_DEFINED
void ndef_thx(pTHX_ struct op *o) {
    o->slabbed = 1;
}
#endif
