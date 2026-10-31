// Step 3: schemas. One declaration gives the struct, parse() with every
// error reported, and to_json().
type Email = string matches email_format;

schema Address {
  city: string where len(self) >= 1;
  zip:  string where len(self) == 7;
}

schema User {
  id:      u64         where self > 0;
  name:    string      where 1 <= len(self) <= 50 transform trim;
  email:   Email;
  age:     int?        where 0 <= self <= 150;
  tags:    vec<string> where len(self) <= 3 = [];
  address: Address?;

  invariant(age == none || age >= 13, "13歳以上");
}

fn greet(u: User) effects(io) {
  print("ようこそ", u.name, "さん", u.to_json());
}

// Raw<Json> is untrusted input; User::parse is the trust boundary.
fn handle(body: Raw<Json>) effects(io, throw) {
  let user = User::parse(body)?;
  greet(user);
}

fn main() effects(io) {
  print(User::parse(parse_json(R"({"id": 1, "name": "  Alice  ", "email": "a@example.com", "age": 30})")));
  print(User::parse(parse_json(R"({
    "id": 0, "name": "", "email": "nope", "age": 200,
    "tags": ["a", 1, "c", "d"], "address": {"city": "", "zip": 123}
  })")));
  print(User::parse(parse_json(R"({"id": 2, "name": "Bob", "email": "b@example.com", "age": 9})")));
  print(User::parse(parse_json("not json")));
}
