// Reduced repro for hitlist-5 Family A: inline-namespace transparency is
// honored by completion (pack_inline_owner_set) but NOT by goto-def
// (member_def_location / pack_member_of) or references (pkg_agrees in
// refs_to). Symbols land under package `v1`; a qualified `mylib::is_thing`
// use carries resolved_package `mylib`, so the package gate drops it.
//
// Mirrors abseil's `ABSL_NAMESPACE_BEGIN` -> `inline namespace head { ... }`
// (absl/base/options.h defines ABSL_OPTION_INLINE_NAMESPACE_NAME = head),
// under which every absl:: symbol is filed as package `head`.
//
// The `#include` makes this file carry a non-empty include closure, so it
// speaks C++'s relative name lookup (like every real corpus TU) — the
// unqualified same-namespace call is then a genuine reference too.
#include "incdep.h"
#define NS_BEGIN inline namespace v1 {
#define NS_END }

namespace mylib {
NS_BEGIN

inline bool is_thing(int c) { return c > 0; }

inline bool caller() {
  bool a = is_thing(1);          // unqualified in-namespace call
  bool b = mylib::is_thing(2);   // qualified call - resolved_package="mylib"
  bool c = mylib::absent(3);     // genuinely-absent member: goto-def must
                                 // FAIL SAFE (no def), never a file-top 1:1
  return a && b && c;
}

NS_END
}  // namespace mylib
