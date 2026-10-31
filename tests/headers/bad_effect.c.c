#include "shared.c.h"

[[cc::effects()]] void loud() { shout(); }

int main() { loud(); }
