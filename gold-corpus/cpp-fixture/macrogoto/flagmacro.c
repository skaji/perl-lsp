#define MY_FEATURE
#ifdef MY_FEATURE
#  define FLAGGED_LIMIT 42
#else
#  define FLAGGED_LIMIT 7
#endif
int flagged_x = FLAGGED_LIMIT;
