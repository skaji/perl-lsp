// Overload set discriminated purely by arity. Goto-def on each call ranks
// the arity-matched overload first (never pruning the rest of the family).
// The last overload is defaulted (3 required, 4 total), so a 3-arg call fits
// it alone.
int pick() { return 0; }
int pick(int a) { return a; }
int pick(int a, int b) { return a + b; }
int pick(int a, int b, int c, int d = 0) { return a + b + c + d; }

int use0() { return pick(); }
int use1() { return pick(1); }
int use2() { return pick(1, 2); }
int use3() { return pick(1, 2, 3); }
