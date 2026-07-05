struct op {
    struct op *op_next;
    unsigned type;
};
typedef struct op OP;
