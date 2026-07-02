// Chained member completion through a substituted return: `cw.get().`
// offers Widget's members because get()'s param-shaped return resolved
// against the receiver's instance args.
struct Widget {
    void spin();
    int weight;
};
template <typename T>
class Crate {
public:
    T get();
    T item_;
};
void go() {
    Crate<Widget> cw;
    cw.get().
}
