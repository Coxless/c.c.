// C.C. runtime: the header every generated .cpp includes.
//
// It can also be used directly from hand-written C++ (step 0 of PLAN.md):
// refinement errors, schema parse errors with field paths, contract checks
// with blame, and the small set of builtins the language exposes.
#pragma once

#include <nlohmann/json.hpp>

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <expected>
#include <fstream>
#include <initializer_list>
#include <map>
#include <optional>
#include <source_location>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

#ifdef CC_TEST
#include <sys/wait.h>
#include <unistd.h>
#endif

namespace cc {

using Json = nlohmann::json;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

// A value did not satisfy the `where` condition of a refined type.
struct RefineError {
  std::string type;       // e.g. "Port", "User.name"
  std::string condition;  // the condition as written in .c.c
};

// One broken field. `path` is "" for the whole value, otherwise "tags[3]" or "addr.city".
struct FieldError {
  std::string path;
  std::string condition;
  std::string message;
};

// Every broken field of a schema parse, not just the first one.
struct ParseError {
  std::string type;
  std::vector<FieldError> errors;

  std::string to_string() const {
    std::string s = type + " の検証に失敗しました（" + std::to_string(errors.size()) + " 件）";
    for (const auto& e : errors) {
      s += "\n  - " + (e.path.empty() ? std::string("(全体)") : e.path) + ": " + e.message;
    }
    return s;
  }
};

// Thrown by `?` when the value is an error. Only functions with `effects(throw)` may do that.
struct Failure : std::runtime_error {
  using std::runtime_error::runtime_error;
};

// ---------------------------------------------------------------------------
// Unvalidated input
// ---------------------------------------------------------------------------

// Data from outside the program. It cannot be used as a validated type until
// it has gone through `T::parse`, which is the trust boundary.
template <class T>
class Raw {
  T v_;

 public:
  explicit Raw(T v) : v_(std::move(v)) {}
  const T& unchecked() const { return v_; }
};

// ---------------------------------------------------------------------------
// Traits for generated types
// ---------------------------------------------------------------------------

template <class T>
concept Refined = requires(const T& t) {
  typename T::underlying_type;
  { T::try_from(std::declval<typename T::underlying_type>()) };
  { t.get() };
};

template <class T>
concept Schema = requires(const T& t, const Json& j, const std::string& p, std::vector<FieldError>& e) {
  { T::parse_at(j, p, e) } -> std::same_as<std::optional<T>>;
  { t.to_json() } -> std::same_as<Json>;
};

template <class T> struct is_vector : std::false_type {};
template <class T, class A> struct is_vector<std::vector<T, A>> : std::true_type {};
template <class T> struct is_optional : std::false_type {};
template <class T> struct is_optional<std::optional<T>> : std::true_type {};
template <class T> struct is_expected : std::false_type {};
template <class T, class E> struct is_expected<std::expected<T, E>> : std::true_type {};

// ---------------------------------------------------------------------------
// Showing values (violation reports and print)
// ---------------------------------------------------------------------------

inline std::string to_display(const FieldError& e);

// Programmer-facing representation: strings are quoted.
template <class T>
std::string repr(const T& v) {
  using U = std::remove_cvref_t<T>;
  if constexpr (std::is_same_v<U, bool>) {
    return v ? "true" : "false";
  } else if constexpr (std::is_same_v<U, std::string> || std::is_same_v<U, std::string_view> ||
                       std::is_same_v<U, const char*> || std::is_array_v<U>) {
    return Json(std::string(v)).dump();
  } else if constexpr (std::is_enum_v<U>) {
    return std::string(cc_enum_names(v)[static_cast<std::size_t>(v)]);
  } else if constexpr (std::is_same_v<U, Json>) {
    return v.dump();
  } else if constexpr (std::is_same_v<U, ParseError>) {
    return v.to_string();
  } else if constexpr (std::is_same_v<U, RefineError>) {
    return v.type + " の条件 " + v.condition + " を満たしていません";
  } else if constexpr (Schema<U>) {
    return v.to_json().dump();
  } else if constexpr (Refined<U>) {
    return repr(v.get());
  } else if constexpr (is_optional<U>::value) {
    return v ? repr(*v) : "none";
  } else if constexpr (is_expected<U>::value) {
    return v ? "ok(" + repr(*v) + ")" : "err(" + repr(v.error()) + ")";
  } else if constexpr (is_vector<U>::value) {
    std::string s = "[";
    for (std::size_t i = 0; i < v.size(); ++i) s += (i ? ", " : "") + repr(v[i]);
    return s + "]";
  } else if constexpr (requires(std::ostream& o) { o << v; }) {
    std::ostringstream o;
    o << v;
    return o.str();
  } else {
    return "<?>";
  }
}

// User-facing representation: strings are printed as-is.
template <class T>
std::string display(const T& v) {
  using U = std::remove_cvref_t<T>;
  if constexpr (std::is_same_v<U, std::string> || std::is_same_v<U, std::string_view> ||
                std::is_same_v<U, const char*> || std::is_array_v<U>) {
    return std::string(v);
  } else if constexpr (Refined<U>) {
    return display(v.get());
  } else {
    return repr(v);
  }
}

// ---------------------------------------------------------------------------
// Contracts
// ---------------------------------------------------------------------------

enum class Blame { caller, callee };
enum class Mode { enforce, observe, off };

#if defined(CC_CONTRACTS_OFF)
inline Mode contract_mode = Mode::off;
#elif defined(CC_CONTRACTS_OBSERVE)
inline Mode contract_mode = Mode::observe;
#else
inline Mode contract_mode = Mode::enforce;
#endif

struct Value {
  std::string name;
  std::string text;
};

// "std::int64_t withdraw(Account&, long int, std::source_location)" -> "withdraw"
inline std::string function_name(const std::source_location& loc) {
  std::string_view s = loc.function_name();
  if (auto p = s.find('('); p != std::string_view::npos) s = s.substr(0, p);
  if (auto p = s.find_last_of(" :*&"); p != std::string_view::npos) s = s.substr(p + 1);
  if (s == "cc_main") s = "main";
  if (s.starts_with("cc_test_")) s = "test";
  return std::string(s);
}

inline std::string where(const std::source_location& loc) {
  std::string file = loc.file_name();
  if (auto p = file.find_last_of('/'); p != std::string::npos) file = file.substr(p + 1);
  return file + ":" + std::to_string(loc.line());
}

// Called when a pre/post condition is false. `caller` is the call site of the
// checked function, `contract` is where the pre/post clause is written.
inline void violation(Blame blame, const char* kind, const char* label, const char* condition,
                      const std::source_location& caller, const std::source_location& contract,
                      std::vector<Value> values) {
  if (contract_mode == Mode::off) return;
  const std::string callee = function_name(contract);
  std::string msg = std::string("contract violation: ") + kind + "「" + label + "」\n";
  msg += std::string("  条件: ") + condition + "\n";
  if (blame == Blame::caller) {
    msg += "  責任: 呼び出し側  " + where(caller) + "  " + function_name(caller) + "() → " + callee + "()\n";
    msg += "  契約: " + where(contract) + "\n";
  } else {
    msg += "  責任: 実装側  " + where(contract) + "  " + callee + "()\n";
    msg += "  呼び出し元: " + where(caller) + "  " + function_name(caller) + "()\n";
  }
  if (!values.empty()) {
    msg += "  値:   ";
    for (std::size_t i = 0; i < values.size(); ++i)
      msg += (i ? ", " : "") + values[i].name + " = " + values[i].text;
    msg += "\n";
  }
  std::fputs(msg.c_str(), stderr);
  std::fflush(stderr);
  if (contract_mode == Mode::enforce) std::abort();
}

}  // namespace cc

// Macros for hand-written C++. Generated code calls cc::violation directly so
// that the condition text in reports is the .c.c source, not the C++ one.
#define CC_V(x) ::cc::Value{#x, ::cc::repr(x)}
#define CC_PRE(cond, label, caller, ...)                                                    \
  do {                                                                                      \
    if (!(cond))                                                                            \
      ::cc::violation(::cc::Blame::caller, "pre", label, #cond, caller,                     \
                      std::source_location::current(), {__VA_ARGS__});                      \
  } while (0)
#define CC_POST(cond, label, caller, ...)                                                   \
  do {                                                                                      \
    if (!(cond))                                                                            \
      ::cc::violation(::cc::Blame::callee, "post", label, #cond, caller,                    \
                      std::source_location::current(), {__VA_ARGS__});                      \
  } while (0)

namespace cc {

// ---------------------------------------------------------------------------
// Parsing (JSON -> validated types)
// ---------------------------------------------------------------------------

inline std::string join(const std::string& path, std::string_view field) {
  return path.empty() ? std::string(field) : path + "." + std::string(field);
}

inline std::string index(const std::string& path, std::size_t i) {
  return path + "[" + std::to_string(i) + "]";
}

inline FieldError refine_failure(const std::string& path, const RefineError& e) {
  return {path, e.condition, e.condition + " を満たしていません"};
}

// Turns an unrefined value into S, recording an error if the condition fails.
template <class S, class U>
std::optional<S> refine_into(U&& u, const std::string& path, std::vector<FieldError>& errs) {
  if constexpr (Refined<S>) {
    auto r = S::try_from(typename S::underlying_type(std::forward<U>(u)));
    if (!r) {
      errs.push_back(refine_failure(path, r.error()));
      return std::nullopt;
    }
    return std::move(*r);
  } else {
    return S(std::forward<U>(u));
  }
}

inline void type_error(const std::string& path, const char* expected, std::vector<FieldError>& errs) {
  errs.push_back({path, std::string("type ") + expected, std::string(expected) + "が必要です"});
}

template <class T>
std::optional<T> decode(const Json& j, const std::string& path, std::vector<FieldError>& errs) {
  if constexpr (Refined<T>) {
    auto u = decode<typename T::underlying_type>(j, path, errs);
    if (!u) return std::nullopt;
    return refine_into<T>(std::move(*u), path, errs);
  } else if constexpr (Schema<T>) {
    return T::parse_at(j, path, errs);
  } else if constexpr (std::is_same_v<T, Json>) {
    return j;
  } else if constexpr (std::is_enum_v<T>) {
    const auto names = cc_enum_names(T{});
    if (j.is_string()) {
      const auto s = j.get<std::string>();
      for (std::size_t i = 0; i < names.size(); ++i)
        if (names[i] == s) return static_cast<T>(i);
    }
    std::string expected;
    for (const auto& n : names) expected += (expected.empty() ? "" : ", ") + std::string(n);
    expected += " のいずれか";
    return type_error(path, expected.c_str(), errs), std::nullopt;
  } else if constexpr (std::is_same_v<T, bool>) {
    if (!j.is_boolean()) return type_error(path, "bool", errs), std::nullopt;
    return j.get<bool>();
  } else if constexpr (std::is_integral_v<T>) {
    if (j.is_number_unsigned()) {
      auto v = j.get<std::uint64_t>();
      if (!std::in_range<T>(v)) return type_error(path, "範囲内の整数", errs), std::nullopt;
      return static_cast<T>(v);
    }
    if (j.is_number_integer()) {
      auto v = j.get<std::int64_t>();
      if (!std::in_range<T>(v)) return type_error(path, "範囲内の整数", errs), std::nullopt;
      return static_cast<T>(v);
    }
    return type_error(path, "整数", errs), std::nullopt;
  } else if constexpr (std::is_floating_point_v<T>) {
    if (!j.is_number()) return type_error(path, "数値", errs), std::nullopt;
    return j.get<T>();
  } else if constexpr (std::is_same_v<T, std::string>) {
    if (!j.is_string()) return type_error(path, "文字列", errs), std::nullopt;
    return j.get<std::string>();
  } else if constexpr (is_optional<T>::value) {
    if (j.is_null()) return T{};
    auto v = decode<typename T::value_type>(j, path, errs);
    if (!v) return std::nullopt;
    return T{std::move(*v)};
  } else if constexpr (is_vector<T>::value) {
    if (!j.is_array()) return type_error(path, "配列", errs), std::nullopt;
    T out;
    bool ok = true;
    for (std::size_t i = 0; i < j.size(); ++i) {
      auto v = decode<typename T::value_type>(j[i], index(path, i), errs);
      if (v) out.push_back(std::move(*v));
      else ok = false;
    }
    if (!ok) return std::nullopt;
    return out;
  } else {
    static_assert(sizeof(T) == 0, "cc::decode: この型は JSON から変換できません");
  }
}

// Root of a schema parse: the input must be a JSON object.
inline bool expect_object(const Json& j, const std::string& path, std::vector<FieldError>& errs) {
  if (j.is_discarded()) {
    errs.push_back({path, "json", "JSON として解釈できません"});
    return false;
  }
  if (!j.is_object()) {
    type_error(path, "オブジェクト", errs);
    return false;
  }
  return true;
}

inline FieldError invariant_failure(const std::string& path, const char* label, const char* condition) {
  std::string msg = label;
  if (msg != condition) msg += "（" + std::string(condition) + "）";
  return {path, condition, msg};
}

inline void missing(const std::string& path, std::vector<FieldError>& errs) {
  errs.push_back({path, "required", "必須フィールドがありません"});
}

// Field lookup that treats JSON null the same as an absent field.
inline const Json* field(const Json& j, const char* name) {
  auto it = j.find(name);
  if (it == j.end() || it->is_null()) return nullptr;
  return &*it;
}

// A field the schema does not declare is usually a typo, so it is an error
// unless the schema is declared `open schema`.
inline void reject_unknown(const Json& j, const std::string& path, std::initializer_list<const char*> known,
                           std::vector<FieldError>& errs) {
  for (const auto& [key, value] : j.items()) {
    bool found = false;
    for (const char* k : known) found = found || key == k;
    if (!found) errs.push_back({join(path, key), "unknown", "未知のフィールドです"});
  }
}

// ---------------------------------------------------------------------------
// Serialization (validated types -> JSON)
// ---------------------------------------------------------------------------

template <class T>
Json encode(const T& v) {
  if constexpr (Schema<T>) {
    return v.to_json();
  } else if constexpr (Refined<T>) {
    return encode(v.get());
  } else if constexpr (std::is_enum_v<T>) {
    return Json(repr(v));
  } else if constexpr (is_optional<T>::value) {
    return v ? encode(*v) : Json(nullptr);
  } else if constexpr (is_vector<T>::value) {
    Json a = Json::array();
    for (const auto& x : v) a.push_back(encode(x));
    return a;
  } else {
    return Json(v);
  }
}

// ---------------------------------------------------------------------------
// `?` and main
// ---------------------------------------------------------------------------

template <class E>
std::string describe(const E& e) {
  return display(e);
}

// The program's arguments, without the program name.
inline std::vector<std::string> command_args(int argc, char** argv) {
  std::vector<std::string> out;
  for (int i = 1; i < argc; ++i) out.emplace_back(argv[i]);
  return out;
}

template <class F>
int run_main(F f) {
  try {
    if constexpr (std::is_void_v<decltype(f())>) {
      f();
      return 0;
    } else {
      return static_cast<int>(f());
    }
  } catch (const std::exception& e) {
    std::fprintf(stderr, "エラー: %s\n", e.what());
    return 1;
  }
}

// ---------------------------------------------------------------------------
// Builtins visible from .c.c
// ---------------------------------------------------------------------------

template <class T>
std::int64_t len(const T& v) {
  if constexpr (Refined<T>) return len(v.get());
  else return static_cast<std::int64_t>(std::size(v));
}

inline std::string trim(std::string s) {
  const char* ws = " \t\r\n";
  auto b = s.find_first_not_of(ws);
  if (b == std::string::npos) return {};
  auto e = s.find_last_not_of(ws);
  return s.substr(b, e - b + 1);
}

// Deliberately simple: one '@', non-empty local part, a dot inside the domain.
inline bool email_format(const std::string& s) {
  auto at = s.find('@');
  if (at == std::string::npos || at == 0 || s.find('@', at + 1) != std::string::npos) return false;
  auto dot = s.find('.', at + 1);
  return dot != std::string::npos && dot > at + 1 && dot + 1 < s.size() &&
         s.find_first_of(" \t\r\n") == std::string::npos;
}

inline Raw<Json> parse_json(const std::string& text) {
  return Raw<Json>(Json::parse(text, nullptr, /*allow_exceptions=*/false));
}

// ---------------------------------------------------------------------------
// Effect handlers (`with io = ...` in tests)
// ---------------------------------------------------------------------------

class IoHandler {
 public:
  virtual ~IoHandler() = default;
  virtual std::string read_file(const std::string& path) = 0;
};

inline thread_local IoHandler* io_handler = nullptr;

// Files in memory. Reading anything else fails like a missing file does.
class MemoryFs : public IoHandler {
  std::map<std::string, std::string> files_;

 public:
  MemoryFs() = default;
  MemoryFs(std::initializer_list<std::pair<const std::string, std::string>> files) : files_(files) {}
  std::string read_file(const std::string& path) override {
    auto it = files_.find(path);
    if (it == files_.end()) throw Failure("ファイルを開けません: " + path);
    return it->second;
  }
};

// Installs a handler for the current scope.
class WithIo {
  IoHandler* previous_;

 public:
  explicit WithIo(IoHandler& handler) : previous_(io_handler) { io_handler = &handler; }
  ~WithIo() { io_handler = previous_; }
  WithIo(const WithIo&) = delete;
  WithIo& operator=(const WithIo&) = delete;
};

inline std::string read_file(const std::string& path) {
  if (io_handler) return io_handler->read_file(path);
  std::ifstream in(path, std::ios::binary);
  if (!in) throw Failure("ファイルを開けません: " + path);
  std::ostringstream ss;
  ss << in.rdbuf();
  return ss.str();
}

template <class... A>
void print(const A&... a) {
  std::string s;
  bool first = true;
  ((s += (first ? "" : " ") + display(a), first = false), ...);
  s += "\n";
  std::fputs(s.c_str(), stdout);
  std::fflush(stdout);
}

template <class... A>
void eprint(const A&... a) {
  std::string s;
  bool first = true;
  ((s += (first ? "" : " ") + display(a), first = false), ...);
  s += "\n";
  std::fputs(s.c_str(), stderr);
}

template <class... A>
void log(const A&... a) {
  std::string s = "[log]";
  ((s += " " + display(a)), ...);
  s += "\n";
  std::fputs(s.c_str(), stderr);
}

// ---------------------------------------------------------------------------
// Tests (`ccc test`)
// ---------------------------------------------------------------------------

#ifdef CC_TEST
inline int test_failures = 0;

inline void expect_failed(const char* condition, const std::source_location& loc, std::vector<Value> values) {
  ++test_failures;
  std::printf("  expect 失敗: %s  (%s)\n", condition, where(loc).c_str());
  for (const auto& v : values) std::printf("    %s = %s\n", v.name.c_str(), v.text.c_str());
}

struct TestCase {
  const char* name;
  void (*run)();
};

// Each test runs in its own process: a contract violation aborts, by design,
// and must not take the remaining tests with it.
inline int run_tests(std::initializer_list<TestCase> tests) {
  std::size_t passed = 0;
  for (const auto& t : tests) {
    std::printf("test %s\n", t.name);
    std::fflush(stdout);
    const pid_t child = fork();
    if (child == 0) {
      try {
        t.run();
      } catch (const std::exception& e) {
        ++test_failures;
        std::printf("  例外: %s\n", e.what());
      }
      std::fflush(stdout);
      std::fflush(stderr);
      _exit(test_failures == 0 ? 0 : 1);
    }
    int status = 0;
    waitpid(child, &status, 0);
    if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
      ++passed;
      std::printf("  ok\n");
    } else if (WIFSIGNALED(status)) {
      std::printf("  FAILED（異常終了）\n");
    } else {
      std::printf("  FAILED\n");
    }
  }
  std::printf("%zu 件中 %zu 件成功\n", tests.size(), passed);
  return passed == tests.size() ? 0 : 1;
}
#endif

}  // namespace cc
