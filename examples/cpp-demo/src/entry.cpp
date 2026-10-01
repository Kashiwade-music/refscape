#include "counter.hpp"

int entry() {
    Counter counter{7};
    return counter.value() + scale(2);
}
