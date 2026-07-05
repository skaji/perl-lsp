#define _SV_HEAD(ptrtype) \
    ptrtype	sv_any;		\
    U32		sv_refcnt;	\
    U32		sv_flags
typedef struct sv SV;
struct sv {
    _SV_HEAD(void*);
};
int use(SV *sv) {
    return sv->sv_flags;
}
