// Extensions in a header, used from another file.
#pragma once
#include <cc/cc.hpp>

#include "nested.c.h"

type Port = u16 where 1 <= self <= 65535;

schema Endpoint {
  host: Host;
  port: Port;
}

[[cc::effects()]]
inline int doubled(int x)
  pre(x >= 0, "負でない")
{
  return x * 2;
}
