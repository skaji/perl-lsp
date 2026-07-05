// Bool-typed C++ expressions (InferredType::Bool across the lattice).
void demo(int x, int y) {
  bool flag = true;      // declared bool         -> Bool
  auto cmp  = x == y;    // comparison expression -> Bool
  int  n    = x + y;     // arithmetic            -> Numeric
  (void)flag; (void)cmp; (void)n;
}
