// Instantiation-aware typing (template arc slice (c)): member types
// substitute the receiver's instance args lazily at query time
// (ReturnExpr::ParamOf / substitute_type_params), the spec-selection
// ladder (exact > partial-pattern > primary) keys member resolution,
// and chains compose through substituted returns.
struct Widget {
    void spin();
    int weight;
};
template <typename T>
class Crate {
public:
    T get();
    auto tail() -> T*;
    T item_;
    int size();
};
template <typename T, typename C> struct codec {
    int parse();
};
template <typename T> struct codec<T*, char> {
    T deref();
    int parse();
};
template <> struct codec<int, char> {
    int whole();
};
void use_them() {
    Crate<int> ci;
    ci.get();
    ci.item_;
    Crate<Widget> cw;
    cw.get().spin();
    cw.item_.spin();
    cw.tail();
    codec<Widget*, char> cp;
    cp.deref().spin();
    cp.parse();
    codec<int, char> ce;
    ce.whole();
    codec<double, double> cd;
    cd.parse();
}
