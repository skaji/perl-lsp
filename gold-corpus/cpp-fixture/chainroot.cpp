class Widget {
public:
    void next();
    int leaf;
};
Widget make_widget() { return Widget(); }
void f() {
    make_widget().next();
}
void g() {
    make_widget().
}
