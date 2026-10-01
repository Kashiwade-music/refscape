#include "counter.hpp"

int entry();

int main() {
    Counter counter{7};
    return entry() == counter.value() + scale(2) ? 0 : 1;
}
