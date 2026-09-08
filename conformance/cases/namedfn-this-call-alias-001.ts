// §22.2.3.{1,3} through an ALIAS. `namedfn_this_value` deliberately
// leaves a `Member` base alone so `foo.call(o)` keeps answering the
// real receiver; a second name for the same declaration was not
// covered by that, so `const g = foo; g.call(o)` promoted to the
// `__forward_foo` cell — the shim whose job is to pin the receiver to
// `undefined` — and silently answered `undefined`.
function foo() {
  return this;
}
const o = { tag: "o" };
const g = foo;
console.log(foo.call(o) === o);
console.log(g.call(o) === o);
console.log(g.apply(o) === o);
console.log(g === foo);
console.log(g.call() === undefined);

function two(a: any, b: any) {
  return String((this as any).tag) + "|" + a + "|" + b;
}
const t2 = two;
console.log(t2.call(o, 1, 2));
console.log(t2.apply(o, [3, 4]));

// the alias is still a value everywhere else: calling it directly
// keeps the receiver-free lane.
console.log(g() === undefined);
