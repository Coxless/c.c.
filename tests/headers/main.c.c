#include "shared.c.h"

int main() {
  cc::print(Endpoint::parse(cc::parse_json(R"({"host": "a", "port": 80})")));
  cc::print(Endpoint::parse(cc::parse_json(R"({"host": "", "port": 0})")));
  cc::print(doubled(2));
  shout();
  cc::print(doubled(-1));
}
