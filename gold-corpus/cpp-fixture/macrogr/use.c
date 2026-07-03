#include "anno.h"
struct counter {
  int mu;
  int waiting GUARD(mu);
  int done GUARD(mu);
};
