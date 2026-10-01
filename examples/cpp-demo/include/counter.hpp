#pragma once
#include "scale.h"

class Counter {
public:
    explicit Counter(int value) : value_(value) {}
    int value() const { return scale(value_); }

private:
    int value_;
};
