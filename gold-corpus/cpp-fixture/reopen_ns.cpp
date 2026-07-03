// hitlist-2 #7 reduction note: a PLAIN reopened namespace (no macro guard)
// already attributes both openings correctly (verified: dropping this
// comment and using bare `namespace d { void f1(); } namespace d { void
// f2(); }` passes today). The real json.hpp/fmt corpus symbols were
// orphaned by a DIFFERENT ingredient: fmt/json wrap every namespace open
// with a macro pair (NLOHMANN_JSON_NAMESPACE_BEGIN/END, FMT_BEGIN_NAMESPACE)
// that this single-file parser can't expand — same root cause as the
// macro-before-class gap (ns_macro.cpp / cpp-xfail-cross-file-namespace-
// macro), generalized to `namespace`: the macro token before `namespace d`
// misparses the whole thing into a bogus function_definition, so even the
// FIRST opening goes dark, not just later reopenings.
NS_BEGIN
namespace d { void f1(); }
NS_END

NS_BEGIN
namespace d { void f2(); }
NS_END
