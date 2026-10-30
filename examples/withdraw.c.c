// Step 2: function contracts and blame.
cpp {
struct Account {
  std::int64_t balance;
};
}

contract fn withdraw(Account& a, i64 amt) -> i64
  effects(log)
  pre(amt > 0, "金額は正")
  pre(a.balance >= amt, "残高が足りる")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{
  a.balance -= amt;
  log("withdraw", amt);
  return amt;
}

// Breaks its own post condition: the report blames the implementation.
contract fn withdraw_buggy(Account& a, i64 amt) -> i64
  pre(amt > 0, "金額は正")
  post(a.balance == old(a.balance) - amt, "残高が amt だけ減る")
{
  a.balance -= amt * 2;
  return amt;
}

extern fn open_account(i64 balance) -> Account effects();
cpp {
inline Account open_account(std::int64_t balance) { return Account{balance}; }
}

fn pay(Account& a, i64 price) effects(io, log) {
  let paid = withdraw(a, price);
  print("paid", paid, "balance", a.balance);
}

fn main() {
  cpp { cc::contract_mode = cc::Mode::observe; }  // report and continue
  let acc = open_account(300);
  pay(acc, 100);
  pay(acc, 500);
  withdraw_buggy(acc, 50);
}
