#include "msg.h"
void handle(union U *u) {
    u->data.ping = 1;
}
