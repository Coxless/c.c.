// Schemas: one declaration gives the struct, parse() with every error reported, make() and to_json().
#include <cc/cc.hpp>

type Email = string matches email_format;

enum Role { admin, member };

schema Address {
  city: string where len(self) >= 1;
  zip:  string where len(self) == 7;
}

schema User {
  id:      u64         where self > 0;
  name:    string      where 1 <= len(self) <= 50 transform trim;
  email:   Email;
  age:     int32?      where 0 <= self <= 150;
  role:    Role = member;
  tags:    vec<string> where len(self) <= 3 = [];
  address: Address?;

  invariant(role != admin || age >= 18, "管理者は18歳以上");
}

void greet(const User& u) {
  cc::print("ようこそ", u.name, "さん", u.to_json());
}

// Raw<Json> is untrusted input; User::parse is the trust boundary.
void handle(cc::Raw<cc::Json> body) {
  User user = cc::must(User::parse(body));
  greet(user);
}

int main() {
  handle(cc::parse_json(R"({"id": 1, "name": "  Alice  ", "email": "a@example.com", "age": 30})"));
  cc::print(User::parse(cc::parse_json(R"({
    "id": 0, "name": "", "email": "nope", "age": 200, "role": "owner",
    "tags": ["a", 1, "c", "d"], "address": {"city": "", "zip": 123}
  })")));
  cc::print(User::parse(cc::parse_json(R"({"id": 2, "name": "Bob", "email": "b@example.com", "role": "admin", "age": 9})")));
  cc::print(User::parse(cc::parse_json("not json")));

  // Values the program has go through the same checks.
  cc::print(User::make({.id = 3, .name = " Carol ", .email = "c@example.com"}));
  cc::print(User::make({.id = 0, .name = "", .email = "c@example.com", .role = Role::admin}));

  // A valid value can be replaced as a whole, but a field cannot be set behind the invariant's back.
  User a = cc::must(User::make({.id = 4, .name = "Dan", .email = "d@example.com"}));
  User b = cc::must(User::make({.id = 5, .name = "Eve", .email = "e@example.com"}));
  a = b;
  cc::print(a.name);
}
