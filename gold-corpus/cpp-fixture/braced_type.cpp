template <class K, class V>
struct FlatMap {
    V at(K k);
    void insert(K k, V v);
};
int main() {
    FlatMap<int, int> m = {{1, 7}, {2, 9}};
    FlatMap<int, int> n;
    FlatMap<int, int> d{{3, 4}};
    return m.at(1);
}
