class Widget {
public:
    void poke();
    int leaf;
};
class Gadget {
public:
    void spin();
};
#define ID(x) (x)
#define CID(x) ((Widget*)(x))
#define SEL2(a, b) (b)
void f() {
    Widget *w;
    Gadget *g;
    w->poke();
    ID(w)->poke();
    CID(w)->poke();
    SEL2(g, w)->poke();
}
