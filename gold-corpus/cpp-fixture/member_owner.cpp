struct alpha {
  int go() { return 1; }
};
struct beta {
  int go() { return 2; }
};
int use_both(alpha a, beta b) {
  int x = a.go();
  return b.go() + x;
}
