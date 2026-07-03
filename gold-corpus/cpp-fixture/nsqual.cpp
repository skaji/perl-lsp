namespace alpha {
int run(int v) { return v + 1; }
}
namespace beta {
int run(int v) { return v + 2; }
}
int use_both() { return alpha::run(1) + beta::run(2); }
