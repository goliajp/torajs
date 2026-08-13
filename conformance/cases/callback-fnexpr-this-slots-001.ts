// Two more no-receiver slots. §7.3.35 GroupBy step 4.c calls the
// grouping callback with `undefined`, and a `then` whose receiver is
// `new Promise(executor)` is still a real promise — the constructor
// desugars to a helper call, so the certainty check has to recognise
// that shape rather than `Expr::New`.

const grouped: any = Object.groupBy([1, 2, 3], function (v: number) {
  console.log("groupBy", typeof this);
  return v % 2 === 0 ? "even" : "odd";
});
console.log(grouped.odd.join(","), grouped.even.join(","));

new Promise(function (resolve: any) {
  // The executor itself already answered `undefined` before this
  // rotation — it arrives through a plain user-fn argument position.
  console.log("executor", typeof this);
  resolve(7);
}).then(function (v: any) {
  console.log("then", typeof this, v);
});
