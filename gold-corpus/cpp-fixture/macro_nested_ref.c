/* A macro whose BODY references another macro. References on the inner macro
 * include the use inside the outer macro's `#define` body — macro bodies are
 * one opaque preproc_arg token, so the macro-body ref lane lexically scans each
 * body for known-macro tokens and mints a read (perl5 `SvFLAGS` used inside
 * `SvOK`/`SvTRUE` etc: was 190/347 real). Family M #3. */
#define FLAGS(x)  (x)->f
#define IS_OK(x)  (FLAGS(x) & 1)

struct s { int f; };

int t(struct s* p) {
    int a = FLAGS(p);
    return IS_OK(p);
}
