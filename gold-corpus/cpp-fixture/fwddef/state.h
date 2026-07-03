#ifndef STATE_H
#define STATE_H
struct GameState { int score; };
extern struct GameState g_state;
int tally(int n);
#endif
