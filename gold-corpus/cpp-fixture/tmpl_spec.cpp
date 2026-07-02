// Class-template specializations: per-spec identity (canonical spelling),
// owned members, the Specializes family (goto-implementation on the primary).
template <typename T, typename Char> struct formatter {
  int parse(int ctx);
};
template <> struct formatter<int, char> {
  int fmt_full();
};
template <typename T> struct formatter<T*, char> {
  int fmt_partial();
};
