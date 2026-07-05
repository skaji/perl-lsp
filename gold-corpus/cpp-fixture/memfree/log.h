#ifndef LOG_H_
#define LOG_H_
struct Logger {
  int info(int x);
};
int info(int x) { return x + 1; }
#endif
