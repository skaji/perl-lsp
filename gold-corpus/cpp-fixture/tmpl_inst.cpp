// Explicit instantiation (the fmt src/format.cc shape): enumerable outline
// items, no top-level parameter leak.
template <typename T> struct Buf {
  void grow(int n);
};
template <typename T> void Buf<T>::grow(int n) { int local_g = n; }
template struct Buf<int>;
template void Buf<float>::grow(int n2);
template <typename T> T sep_impl(T loc);
template int sep_impl(int loc);
