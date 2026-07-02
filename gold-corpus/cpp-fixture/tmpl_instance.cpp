// Member access on a template INSTANCE (`Crate<Payload> c; c.size()`):
// the declared spelling peels to ParametricType::Instance, dispatch keys
// the BASE, so members resolve through the plain-class machinery. A
// typedef landing on a template spelling chases to the same Instance.
// An instance whose base has a per-spec class keys members off the exact
// canonical spelling (`codec<int>`); no spec -> the base primary.
struct Payload {
    void spin();
};

template <typename T>
class Crate {
public:
    T get();
    int size();
    T item_;
};

typedef Crate<int> IntCrate;

template <typename T> struct codec {
    void parse();
};
template <> struct codec<int> {
    void pack_int();
};

void use_crate() {
    Crate<Payload> c;
    c.size();
    c.get();
    IntCrate ic;
    ic.size();
    codec<int> ci;
    codec<char> cc;
    ci.pack_int();
    cc.parse();
}
