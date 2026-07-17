// Out-of-line definitions whose owning class is declared elsewhere (a header),
// written inside a namespace — hitlist H7-2 / re2 F4/F7/F1/F8. The `::`
// qualifier names the owner, so each method attributes to its CLASS, never the
// enclosing `app` namespace.
namespace app {

// (a) pointer return — an extra pointer_declarator wraps the function_declarator.
Regexp* Regexp::Simplify() { return 0; }

// (b) multi-level qualifier — the qualified_identifier nests (owner = inner Inst).
void Prog::Inst::InitAlt(int a) { }

// (c) out-of-line constructor — no return type at all.
RE2::RE2(const char* pattern) { }

// (b+c) multi-level out-of-line constructor.
RE2::Options::Options(int flags) { }

}  // namespace app
