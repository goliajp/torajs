// proposal-upsert's `getOrInsertComputed` computes the missing value
// with `Call(callbackfn, undefined, «key»)`, and §7.3.35 GroupBy step
// 4.c calls its callback the same way. Both need a receiver the
// program can only have gotten from the builtin constructor — a user
// object with a method of that name binds the callback however it
// likes.
//
// `var m = new Map()` is that receiver as surely as `const m` is: the
// slot is written once and the name is bound nowhere else. Before the
// census widened, only the `const` spelling reached the table.

var m: any = new Map();

console.log(
  m.getOrInsertComputed("a", function (k: any) {
    return "computed:" + k + ":" + typeof this;
  }),
);
console.log(m.get("a"));

// present key — the callback does not run at all
console.log(m.getOrInsertComputed("a", function () {
  return "second-call";
}));

var wm: any = new WeakMap();
var key: any = {};
console.log(
  wm.getOrInsertComputed(key, function () {
    return this === undefined ? "no-receiver" : "receiver";
  }),
);

// `Map.groupBy` names its callback slot through the static, not
// through a receiver binding — it is here to show the two lanes agree
var items: any = [1, 2, 3, 4];
var grouped: any = Map.groupBy(items, function (v: any) {
  return this === undefined ? (v % 2 === 0 ? "even" : "odd") : "wrong";
});
console.log(grouped.get("odd").join(","), grouped.get("even").join(","));
