#include "multitu/strjoin.h"
int test_combine() {
  pkg::Mutex m;
  m.Lock();
  return pkg::Combine(5, 6);
}
