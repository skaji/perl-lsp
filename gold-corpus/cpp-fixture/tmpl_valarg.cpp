enum class StatusCode {
  kOk,
  kNotFound,
};

const int BUF_LIMIT = 64;

template <StatusCode C>
int MakeError(int x) {
  return x;
}

template <int N>
struct Buffer {
  int data[N];
};

int main() {
  int e = MakeError<StatusCode::kNotFound>(1);
  Buffer<BUF_LIMIT> buf;
  StatusCode plain = StatusCode::kNotFound;
  return e + buf.data[0];
}

namespace outer {
enum class Mode {
  kFast,
  kSlow,
};
}

template <outer::Mode M>
int Run(int x) {
  return x;
}

int run_it() {
  return Run<outer::Mode::kSlow>(2);
}
