struct CallW {
    int slot;
    int other;
};
typedef struct CallW CallW;

CallW *cfMkStruct(void);

int cf_use(void) {
    CallW *w = cfMkStruct();
    int a = w->slot;
    int b = cfMkStruct()->slot;
    return a + b;
}
