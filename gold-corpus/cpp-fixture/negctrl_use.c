/* Over-linking negative control: this TU deliberately does NOT #include
   "negctrl_other.h", where `NcThingB *ncMakeThing(void)` (with a `secret`
   field) is declared. The include closure does not reach it, so the callee's
   return type must NOT cross the boundary — `ncMakeThing()->secret` stays
   unresolved. Gd on `secret` must land on nothing, not on the unrelated
   same-named function's struct. */
int nc_use(void) {
    return ncMakeThing()->secret;
}
