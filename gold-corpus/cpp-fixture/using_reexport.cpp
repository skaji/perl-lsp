// hitlist-2 #8: `using Base::insert;` re-exports Base's member into
// Derived's public API. Verified NOT reduced for definition — gd from
// `x.insert()` already reaches Base::insert correctly, not Other::insert
// (the unrelated same-named plant). What IS broken: the re-export is
// invisible in outline (no `insert` entry nests under Derived), and hover
// on the same call resolves to the WRONG bare-name match (`Other`) even
// though gd on the identical position is right — a hover/gd disagreement
// on one symbol (hitlist's cross-verb theme, #14).
struct Other { void insert(); };
struct Base { void insert(); };
struct Derived : Base { using Base::insert; };
void use_it() {
    Derived x;
    x.insert();
}
