#define _HEAD_A(p) \
    p	a_any;		\
    unsigned	a_flags
#define _HEAD_B(p) \
    p	b_any;		\
    unsigned	b_flags
typedef struct myagg AG;
struct myagg {
    _HEAD_A(void*);
    _HEAD_B(void*);
};
int use(AG *g) {
    return g->a_flags + g->b_flags;
}
