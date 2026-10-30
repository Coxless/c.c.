// Step 0: the target shape for a `schema`, written by hand.
//
//   type Email = string matches email_format;
//   schema User {
//     id:    u64         where self > 0;
//     name:  string      where 1 <= len(self) <= 50 transform trim;
//     email: Email;
//     age:   i64?        where 0 <= self <= 150;
//     tags:  vec<string> where len(self) <= 3 = [];
//     invariant(age == none || age >= 13, "13歳以上");
//   }
#include <cc/cc.hpp>

class Email {
 public:
  using underlying_type = std::string;
  static bool check(const std::string& self) { return cc::email_format(self); }
  static std::expected<Email, cc::RefineError> try_from(std::string v) {
    if (!check(v)) return std::unexpected(cc::RefineError{"Email", "matches email_format"});
    return Email(std::move(v));
  }
  const std::string& get() const { return v_; }
  operator const std::string&() const { return v_; }

 private:
  explicit Email(std::string v) : v_(std::move(v)) {}
  std::string v_;
};

struct User {
  // One refined type per field that has a `where` clause.
  class id_t {
   public:
    using underlying_type = std::uint64_t;
    static constexpr bool check(const std::uint64_t& self) { return self > 0; }
    static constexpr std::expected<id_t, cc::RefineError> try_from(std::uint64_t v) {
      if (!check(v)) return std::unexpected(cc::RefineError{"User.id", "self > 0"});
      return id_t(v);
    }
    constexpr const std::uint64_t& get() const { return v_; }
    constexpr operator const std::uint64_t&() const { return v_; }

   private:
    explicit constexpr id_t(std::uint64_t v) : v_(v) {}
    std::uint64_t v_;
  };

  class name_t {
   public:
    using underlying_type = std::string;
    static bool check(const std::string& self) { return 1 <= cc::len(self) && cc::len(self) <= 50; }
    static std::expected<name_t, cc::RefineError> try_from(std::string v) {
      if (!check(v)) return std::unexpected(cc::RefineError{"User.name", "1 <= len(self) <= 50"});
      return name_t(std::move(v));
    }
    const std::string& get() const { return v_; }
    operator const std::string&() const { return v_; }

   private:
    explicit name_t(std::string v) : v_(std::move(v)) {}
    std::string v_;
  };

  class age_t {
   public:
    using underlying_type = std::int64_t;
    static constexpr bool check(const std::int64_t& self) { return 0 <= self && self <= 150; }
    static constexpr std::expected<age_t, cc::RefineError> try_from(std::int64_t v) {
      if (!check(v)) return std::unexpected(cc::RefineError{"User.age", "0 <= self <= 150"});
      return age_t(v);
    }
    constexpr const std::int64_t& get() const { return v_; }
    constexpr operator const std::int64_t&() const { return v_; }

   private:
    explicit constexpr age_t(std::int64_t v) : v_(v) {}
    std::int64_t v_;
  };

  class tags_t {
   public:
    using underlying_type = std::vector<std::string>;
    static bool check(const std::vector<std::string>& self) { return cc::len(self) <= 3; }
    static std::expected<tags_t, cc::RefineError> try_from(std::vector<std::string> v) {
      if (!check(v)) return std::unexpected(cc::RefineError{"User.tags", "len(self) <= 3"});
      return tags_t(std::move(v));
    }
    const std::vector<std::string>& get() const { return v_; }
    operator const std::vector<std::string>&() const { return v_; }

   private:
    explicit tags_t(std::vector<std::string> v) : v_(std::move(v)) {}
    std::vector<std::string> v_;
  };

  id_t id;
  name_t name;
  Email email;
  std::optional<age_t> age;
  tags_t tags;

  bool invariant_0() const { return age == std::nullopt || age >= 13; }

  static std::optional<User> parse_at(const cc::Json& j, const std::string& path,
                                      std::vector<cc::FieldError>& errs) {
    if (!cc::expect_object(j, path, errs)) return std::nullopt;
    const auto n = errs.size();

    std::optional<id_t> f_id;
    if (auto* v = cc::field(j, "id")) {
      if (auto d = cc::decode<std::uint64_t>(*v, cc::join(path, "id"), errs))
        f_id = cc::refine_into<id_t>(std::move(*d), cc::join(path, "id"), errs);
    } else {
      cc::missing(cc::join(path, "id"), errs);
    }

    std::optional<name_t> f_name;
    if (auto* v = cc::field(j, "name")) {
      if (auto d = cc::decode<std::string>(*v, cc::join(path, "name"), errs)) {
        *d = cc::trim(std::move(*d));  // transform runs before the condition
        f_name = cc::refine_into<name_t>(std::move(*d), cc::join(path, "name"), errs);
      }
    } else {
      cc::missing(cc::join(path, "name"), errs);
    }

    std::optional<Email> f_email;
    if (auto* v = cc::field(j, "email")) {
      f_email = cc::decode<Email>(*v, cc::join(path, "email"), errs);
    } else {
      cc::missing(cc::join(path, "email"), errs);
    }

    std::optional<age_t> f_age;  // optional field: absent is fine
    if (auto* v = cc::field(j, "age")) {
      if (auto d = cc::decode<std::int64_t>(*v, cc::join(path, "age"), errs))
        f_age = cc::refine_into<age_t>(std::move(*d), cc::join(path, "age"), errs);
    }

    std::optional<tags_t> f_tags;
    if (auto* v = cc::field(j, "tags")) {
      if (auto d = cc::decode<std::vector<std::string>>(*v, cc::join(path, "tags"), errs))
        f_tags = cc::refine_into<tags_t>(std::move(*d), cc::join(path, "tags"), errs);
    } else {
      f_tags = cc::refine_into<tags_t>(std::vector<std::string>{}, cc::join(path, "tags"), errs);
    }

    if (errs.size() != n) return std::nullopt;
    User out{std::move(*f_id), std::move(*f_name), std::move(*f_email), std::move(f_age),
             std::move(*f_tags)};
    if (!out.invariant_0()) errs.push_back(cc::invariant_failure(path, "13歳以上", "age == none || age >= 13"));
    if (errs.size() != n) return std::nullopt;
    return out;
  }

  static std::expected<User, cc::ParseError> parse(const cc::Json& j) {
    std::vector<cc::FieldError> errs;
    auto v = parse_at(j, "", errs);
    if (!v) return std::unexpected(cc::ParseError{"User", std::move(errs)});
    return std::move(*v);
  }
  static std::expected<User, cc::ParseError> parse(const cc::Raw<cc::Json>& raw) {
    return parse(raw.unchecked());
  }

  cc::Json to_json() const {
    cc::Json j = cc::Json::object();
    j["id"] = cc::encode(id);
    j["name"] = cc::encode(name);
    j["email"] = cc::encode(email);
    j["age"] = cc::encode(age);
    j["tags"] = cc::encode(tags);
    return j;
  }
};

int main() {
  auto good = User::parse(cc::parse_json(R"({"id": 1, "name": "  Alice  ", "email": "a@example.com", "age": 30})"));
  cc::print(good);

  // Every broken field is reported, not only the first.
  auto bad = User::parse(cc::parse_json(
      R"({"id": 0, "name": "", "email": "nope", "age": 200, "tags": ["a", 1, "c", "d"]})"));
  cc::print(bad);

  auto young = User::parse(cc::parse_json(R"({"id": 2, "name": "Bob", "email": "b@example.com", "age": 9})"));
  cc::print(young);

  cc::print(User::parse(cc::parse_json("not json")));
}
