// Function contracts with blame, written by hand with the macros of cc.hpp: plain C++.
//
//   std::int64_t withdraw(Account& a, std::int64_t amt)
//     pre(amt > 0, "金額は正")
//     pre(a.balance >= amt, "残高が足りる")
//     post(r: r == amt)
//     post(a.balance == old(a.balance) - amt)
#include <cc/cc.hpp>

struct Account {
  std::int64_t balance;
};

std::int64_t withdraw(Account& a, std::int64_t amt,
                      std::source_location caller = std::source_location::current()) {
  CC_PRE(amt > 0, "金額は正", caller, CC_V(amt));
  CC_PRE(a.balance >= amt, "残高が足りる", caller, CC_V(a.balance), CC_V(amt));
  const auto old_balance = a.balance;  // old(a.balance)

  a.balance -= amt;
  const std::int64_t r = amt;

  CC_POST(r == amt, "r == amt", caller, CC_V(r), CC_V(amt));
  CC_POST(a.balance == old_balance - amt, "a.balance == old(a.balance) - amt", caller,
          CC_V(a.balance), CC_V(old_balance), CC_V(amt));
  return r;
}

// An implementation that breaks its own post condition.
std::int64_t withdraw_buggy(Account& a, std::int64_t amt,
                            std::source_location caller = std::source_location::current()) {
  CC_PRE(amt > 0, "金額は正", caller, CC_V(amt));
  const auto old_balance = a.balance;
  a.balance -= amt * 2;
  CC_POST(a.balance == old_balance - amt, "a.balance == old(a.balance) - amt", caller,
          CC_V(a.balance), CC_V(old_balance), CC_V(amt));
  return amt;
}

int main() {
  cc::contract_mode = cc::Mode::observe;  // report and continue, so both cases show
  Account acc{300};
  cc::print("withdraw(100) ->", withdraw(acc, 100), "balance", acc.balance);
  withdraw(acc, 500);         // caller's fault
  withdraw_buggy(acc, 50);    // implementation's fault
}
