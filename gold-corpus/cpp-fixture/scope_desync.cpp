// Minimal reduction of hitlist-2 #6: a macro-attributed move ctor with a
// multi-line noexcept(... && ...) clause and a brace-init nested inside a
// ternary member-initializer desyncs scope tracking for everything after it.
class Widget {
 public:
  ATTR_NOINLINE Widget(Widget&& that) noexcept(
      std::is_nothrow_copy_constructible_v<int> &&
      std::is_nothrow_copy_constructible_v<int>)
      : settings_(cond
                      ? a
                      : Common{tag{},
                               x},
                  that.hash_ref(), that.eq_ref(), that.char_alloc_ref()) {
    that.x_ = 0;
  }

  int get() && { return 0; }

 private:
  int cache_;

 public:
  void after();
};
