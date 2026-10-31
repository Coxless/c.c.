// Function contracts and blame: a broken `pre` is the caller's fault, a broken `post` the implementation's.
#include <cc/cc.hpp>

#include <cstdint>
#include <iostream>

struct Account {
  std::int64_t balance;
};

std::int64_t withdraw(Account& a, std::int64_t amt)
  pre(amt > 0, "金額は正")
  pre(a.balance >= amt, "残高が足りる")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{
  a.balance -= amt;
  cc::log("withdraw", amt);
  return amt;
}

// Breaks its own post condition: the report blames the implementation.
std::int64_t withdraw_buggy(Account& a, std::int64_t amt)
  pre(amt > 0, "金額は正")
  post(a.balance == old(a.balance) - amt, "残高が amt だけ減る")
{
  a.balance -= amt * 2;
  return amt;
}

void pay(Account& a, std::int64_t price) {
  auto paid = withdraw(a, price);
  std::cout << "paid " << paid << " balance " << a.balance << "\n";
}

int main() {
  cc::contract_mode = cc::Mode::observe;  // report and continue
  Account acc{300};
  pay(acc, 100);
  pay(acc, 500);
  withdraw_buggy(acc, 50);
}
