#define _SV_HEAD(ptrtype) \
    ptrtype	sv_any;		\
    unsigned	sv_refcnt;	\
    unsigned	sv_flags
struct STRUCT_SV {
    _SV_HEAD(void*);
};
