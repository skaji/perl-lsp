enum fruit { APPLE, BANANA, CHERRY };
struct basket { int kind; };
struct crate { int kind; };
int ba(struct basket* b) { return b->kind == APPLE; }
int bb(struct basket* b) { return b->kind == BANANA; }
int bc(struct basket* b) { return b->kind == CHERRY; }
int ca(struct crate* c) { return c->kind == 3; }
int cb(struct crate* c) { return c->kind == 7; }
