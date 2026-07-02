// thousands_sep_result-shaped regression net (the fmt anchor, hermetic):
// a template struct named in trailing-return positions across two files.
template <typename Char> struct sep_result {
  Char thousands;
};
template <typename Char>
auto sep_lookup(int loc) -> sep_result<Char>;
