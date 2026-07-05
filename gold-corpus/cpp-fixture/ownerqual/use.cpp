#include "kind.h"
int classify(dynamic::Type k) {
  switch (k) {
    case dynamic::STRING: return 1;
    default: return 0;
  }
}
int is_str(dynamic::Type k) {
  return k == dynamic::STRING;
}
