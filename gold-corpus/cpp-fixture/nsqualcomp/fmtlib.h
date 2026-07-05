#pragma once

namespace fmtx {
void format_to(int v);
void print(int v);
namespace detail {
void detail_helper();
}
inline namespace v11 {
void inline_fn();
}
}

// A similarly-named global that must NOT leak into `fmtx::` completion.
void formatter_global(int v);
