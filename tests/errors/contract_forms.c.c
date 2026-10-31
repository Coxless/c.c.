#include <string>
// Not supported yet: each of these is an error rather than a silently unchecked contract.
template <class T>
T tmpl(T x) pre(x > 0) { return x; }
struct S {
  int method(int x) const pre(x > 0) { return x; }
};
int S_out(int x) pre(x > 0);
int variadic(int a, ...) pre(a > 0) { return a; }
int unnamed(int) pre(true) { return 0; }
auto deduced(int x) pre(x > 0) { return x; }
int main() pre(true) { return 0; }
void f() {
  type Local = int where self > 0;
}
