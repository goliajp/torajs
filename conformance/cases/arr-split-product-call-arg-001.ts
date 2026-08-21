// A fresh split product handed straight to an owned-string parameter
// — `f(s.split(" "))` with `f(xs: string[])` — used to arrive as the
// view array: the callee decoded the inline views as owned strings
// (`JSON.stringify` / `.at` / for-of printed garbage) while the
// flag-routed readers (`join`, `===`) happened to agree. Plan-state
// 469-01, pre-existing. The call-argument boundary now applies the
// copy-out rule: a fresh product is materialized in place, a borrowed
// one copied out and released after the call. Direct call, fn-value
// call, setter, and a method, each on a heap parent that dies first,
// with churn before the read. Rotation 469.

function show(tag: string, a: string[]) {
  console.log(tag, a.length, JSON.stringify(a), a.join("|"), a.at(0), a[1] === "q");
  for (const x of a) console.log(" ", x, x.length);
}
function keep(a: string[]): string[] { a.push("end" + "!"); return a; }

show("literal-direct", "p q r".split(" "));
let h = "p q r" + "!";
show("heap-direct", h.split(" "));

const kept = keep(("a b c" + "?").split(" "));
let junk: string[] = [];
for (let i = 0; i < 64; i++) junk.push("zz" + i);
show("kept", kept);

// fn-value lane
const f = (xs: string[]): string => JSON.stringify(xs) + xs.join("+");
console.log(f("m n o".split(" ")));
console.log(f(("m n o" + "!").split(" ")));

// setter
class Box {
  _v: string[] = [];
  set v(xs: string[]) { this._v = xs; }
  get v(): string[] { return this._v; }
}
const b = new Box();
b.v = ("u v w" + "!").split(" ");
let junk2: string[] = [];
for (let i = 0; i < 64; i++) junk2.push("yy" + i);
console.log(JSON.stringify(b.v), b.v.join("|"));

// method
class Sink {
  items: string[] = [];
  add(xs: string[]) { for (const x of xs) this.items.push(x); }
}
const s = new Sink();
s.add(("k l" + "!").split(" "));
s.add("m n".split(" "));
console.log(JSON.stringify(s.items), s.items.length);

// the binding form, for contrast: already covered by the census
const viaBinding = ("x y" + "?").split(" ");
show("binding", viaBinding);
