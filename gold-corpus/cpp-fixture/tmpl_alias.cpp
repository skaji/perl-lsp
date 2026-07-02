// `using` aliases (plain + template) mint findable type symbols.
using byte_alias = unsigned char;
template <typename T> struct Buf { int n_; };
template <typename T> using vec_alias = Buf<T>;
