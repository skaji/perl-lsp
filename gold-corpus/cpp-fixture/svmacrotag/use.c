#include "perlish.h"
unsigned via_tag(struct STRUCT_SV *s) {
    return s->sv_flags;
}
