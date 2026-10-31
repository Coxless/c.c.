#include <cc/cc.hpp>
enum Role { admin, member, admin };
schema S {
  a: i64 where helper(self) > 0;
  role: Role = owner;
  invariant(role == owner, "x");
  invariant(role == Role::owner, "y");
}
type T = i64 where self > 0 + nope;
schema Dup { a: i64; a: i64; }
schema Dup { b: i64; }
schema Holds { body: Raw<Json>; }
