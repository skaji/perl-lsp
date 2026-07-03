#include "state.h"
static int helper(int n);
struct GameState g_state;
int tally(int n) {
    g_state.score += helper(n);
    return g_state.score;
}
static int helper(int n) { return n * 2; }
