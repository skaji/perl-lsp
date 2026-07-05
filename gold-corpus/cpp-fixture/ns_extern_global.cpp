namespace lib {
namespace detail {
extern const unsigned char kTable[256];
extern int kCounter;
}
int probe(int c) {
    return detail::kTable[c] + detail::kCounter;
}
}
