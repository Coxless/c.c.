fn helper(x: i64) -> i64 effects() { return x; }
enum Role { admin, member, admin }
schema S {
  a: i64 where helper(self) > 0;
  role: Role = owner;
  invariant(role == owner, "x");
  invariant(role == Role::owner, "y");
}
type T = i64 where self > 0;
fn g(r: Role) effects() {}
fn h() effects() { g(nobody); }
fn main(n: i64) {}
