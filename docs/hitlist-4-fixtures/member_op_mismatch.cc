// Mode-B (member-access-operator diagnostic) controls, hitlist-4 family D.
// Server publishDiagnostics flags `o.op_type` ("use `->` here"); the DEEP
// receiver `p` (OP**) is skipped by design (expected_member_op → None), so
// veesh's "you need to peel" hint never fires for `op_p->` / `op_p.`.
// The CLI --batch diagnostics path omits pack_member_op_diagnostics entirely
// (pack_diagnostics is wired only in backend publish) — probes can't see Mode B.
#include "op_mini.h"
void ctrl_m(OP* o) {
    o.op_type;
}
void deep_n(OP** p) {
    p->op_type;
}
