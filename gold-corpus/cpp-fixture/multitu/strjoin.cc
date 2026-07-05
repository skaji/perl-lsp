#include "multitu/strjoin.h"
namespace pkg {
int Combine(int a, int b) {
  Mutex m;
  m.Lock();
  return a + b;
}
void Mutex::Lock() {}
void Mutex::Unlock() {}
}  // namespace pkg
