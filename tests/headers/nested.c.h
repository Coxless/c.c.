#pragma once
#include <cc/cc.hpp>

#include "plain.h"

constexpr int min_host = PLAIN_MIN_HOST;
type Host = string where len(self) >= 1;

[[cc::effects(io)]] inline void shout() { cc::print("nested"); }
