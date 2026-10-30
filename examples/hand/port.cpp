// Step 0: the target shape for `type Port = u16 where 1 <= self <= 65535;`
// (CONCEPT.md 9.4), written by hand.
#include <cc/cc.hpp>

class Port {
 public:
  using underlying_type = std::uint16_t;

  static constexpr bool check(const std::uint16_t& self) { return 1 <= self && self <= 65535; }

  static constexpr std::expected<Port, cc::RefineError> try_from(std::uint16_t v) {
    if (!check(v)) return std::unexpected(cc::RefineError{"Port", "1 <= self <= 65535"});
    return Port(v);
  }

  // Literals: the condition is checked at compile time and no runtime check is left.
  template <std::uint16_t V>
    requires(check(V))
  static consteval Port of() {
    return Port(V);
  }

  constexpr const std::uint16_t& get() const { return v_; }
  constexpr operator const std::uint16_t&() const { return v_; }
  friend constexpr bool operator==(const Port&, const Port&) = default;
  friend constexpr auto operator<=>(const Port&, const Port&) = default;

 private:
  explicit constexpr Port(std::uint16_t v) : v_(v) {}
  std::uint16_t v_;
};

int main() {
  constexpr Port http = Port::of<8080>();
  static_assert(http.get() == 8080);
#ifdef CC_EXPECT_COMPILE_ERROR
  Port::of<0>();  // 0 does not satisfy 1 <= self: rejected at compile time
#endif
  cc::print("literal:", http);
  cc::print("try_from(443):", Port::try_from(443));
  cc::print("try_from(0):", Port::try_from(0));
}
