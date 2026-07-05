#include "multitu/strjoin.h"
int run_app() {
  pkg::Mutex mu;
  mu.Lock();
  return pkg::Combine(1, 2);
}
