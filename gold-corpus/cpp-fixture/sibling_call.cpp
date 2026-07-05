void freefunc();

template <class T>
struct Buf {
    void grow(int n);
    void reserve(int n);
};
template <class T>
void Buf<T>::reserve(int n) { grow(n); }

struct Widget {
    void paint();
    int width();
    void render() { paint(); }
    void resize();
};
void Widget::resize() { width(); }

int helper();
struct Gadget {
    void run() { helper(); }
};

struct Board {
    void freefunc();
    void tick() { freefunc(); }
};
