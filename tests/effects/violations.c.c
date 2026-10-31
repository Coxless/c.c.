// Every kind of call the check has to see, including the ones that are not written.
#include <cc/cc.hpp>

#include <functional>
#include <string>
#include <vector>

[[cc::effects(io)]] void log_line();
void plain();

struct Conv {
  [[cc::effects(io)]] Conv(int);
};
void take(Conv);

struct Widget {
  [[cc::effects(io)]] ~Widget();
};

struct Op {
  [[cc::effects(io)]] Op operator+(const Op&) const;
};

namespace ns {
struct A {};
[[cc::effects(io)]] void adl(A);
}  // namespace ns

template <class T>
[[cc::effects(io)]] void tmpl(T);

#define VIA_MACRO() log_line()

struct V {
  [[cc::effects()]] virtual void m();
};

[[cc::effects()]] void direct() { log_line(); }
[[cc::effects()]] void undeclared() { plain(); }
[[cc::effects()]] void implicit_ctor() { take(1); }
[[cc::effects()]] void implicit_dtor() { Widget w; }
[[cc::effects()]] Op op(Op a, Op b) { return a + b; }
[[cc::effects()]] void template_instance() { tmpl(1); }
[[cc::effects()]] void via_adl() { ns::A a; adl(a); }
[[cc::effects()]] void via_macro() { VIA_MACRO(); }
[[cc::effects()]] void in_lambda() { auto f = [] { log_line(); }; f(); }
[[cc::effects()]] void virtual_call(V& v) { v.m(); }
[[cc::effects()]] void pointer_call(void (*f)()) { f(); }
[[cc::effects()]] void function_object(std::function<void()>& f) { f(); }
[[cc::effects()]] void allocates() { int* p = new int(1); delete p; }
[[cc::effects()]] void throws() { throw 1; }
[[cc::effects()]] void reinterprets(int* p) { (void)reinterpret_cast<long*>(p); }
[[cc::effects()]] void push(std::vector<int>& v) { v.push_back(1); }
[[cc::effects()]] void at_throws(std::vector<int>& v) { (void)v.at(0); }
[[cc::effects()]] void string_temp() { std::string s = "text"; }
[[cc::effects()]] void prints() { cc::print("x"); }
[[cc::effects(alloc)]] void alloc_is_not_io() { cc::print("x"); }
[[cc::effects(throw)]] void must_needs_alloc() { (void)cc::parse_json("1"); }

// A contract's conditions are part of what the function does.
[[cc::effects(io)]] bool has_io();
[[cc::effects()]] int contracted(int x) pre(has_io()) { return x; }

// Declared first, defined later: the definition is held to the declaration.
[[cc::effects()]] void declared_first();
void declared_first() { log_line(); }

// A try block with `catch (...)` keeps `throw` in; any narrower catch does not.
[[cc::effects()]] void not_all_caught() {
  try {
    throw 1;
  } catch (const std::string&) {
  }
}

// One error per template instance is not needed: instances of an annotated template are checked.
template <class T>
[[cc::effects()]] void annotated_template(T t) { take(t); }
void instantiate() { annotated_template(1); }

// A member function defined outside its class is held to its declaration.
struct Counter {
  [[cc::effects()]] int next();
};
int Counter::next() { log_line(); return 1; }
