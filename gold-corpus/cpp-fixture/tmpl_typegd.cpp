template <typename T> struct fmtr {
  int generic;
};
template <> struct fmtr<int> {
  int special;
};
struct user : fmtr<int> {
  int extra;
};
