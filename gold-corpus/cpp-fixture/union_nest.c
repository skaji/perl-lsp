/* op.h-shaped unions: members nest under their container in the outline,
 * stay flat on the struct for completion/refs, and hover shows the overlay. */
struct OPX { int dummy; };
struct pmop2 {
  int op_flags;
  union {
    struct OPX *op_pmreplroot;
    unsigned long op_pmtargetoff;
  } op_pmreplrootu;
  union {
    long anon_a;
    char anon_b;
  };
};
void poke(struct pmop2 *pm) {
  pm->op_pmreplrootu;
}
