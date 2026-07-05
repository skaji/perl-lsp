// Member-access operator DX (hitlist-4 Family D). Self-contained: no includes,
// so the whole-workspace diagnostics run over this dir is deterministic.
struct OP { int op_type; };

void ctrl(OP* o) {
    o.op_type;      // Mode B swap: `o` is OP*, wrote `.`, wants `->`.
}

void deep(OP** op_p) {
    op_p->op_type;  // DEEP peel: `op_p` is OP**, no single token reaches it —
                    // wants `(*op_p)->`. Show-only (no auto-fix).
}
