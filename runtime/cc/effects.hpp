// What `[[cc::effects(...)]]` turns into, for a file that uses nothing else of C.C.
//
// The effect check runs under Clang and reads these from its AST. Other compilers
// see nothing, so they have no unknown attribute to warn about.
#pragma once

#if defined(__clang__)
// A function whose body the effect check holds to this list.
#define CC_EFFECTS(list) clang::annotate("cc", "effects:" list)
// A function the check takes at its word: generated code and this library.
#define CC_TRUSTED(list) [[clang::annotate("cc", "trusted:" list)]]
// A class whose implicit special members are known not to do anything but copy its members.
#define CC_GENERATED [[clang::annotate("cc", "generated")]]
#else
#define CC_EFFECTS(list)
#define CC_TRUSTED(list)
#define CC_GENERATED
#endif
