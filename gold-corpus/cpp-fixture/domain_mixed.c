enum opcode2 { OP2_A, OP2_B, OP2_C };
struct node { int tag; };
int probe(struct node* n) {
    if (n->tag == OP2_B) return 1;
    n->tag = 5;
    n->tag = 6;
    return 0;
}
