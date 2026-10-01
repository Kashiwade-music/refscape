#include "scale.h"

#ifndef REFSCAPE_DEMO_FACTOR
#error "Configure the project to supply REFSCAPE_DEMO_FACTOR"
#endif

int scale(int value) {
    return value * REFSCAPE_DEMO_FACTOR;
}
