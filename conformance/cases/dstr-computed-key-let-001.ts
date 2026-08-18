// §14.3.3 ComputedPropertyName in declaration-position object
// destructuring — `let/const/var { [expr]: binding } = src`.
const k1 = "a";
const { [k1]: v1 } = { a: 1 };
console.log(v1);

// composite key expression
const { ["x" + "y"]: v2 } = { xy: 2 };
console.log(v2);

// default fires on a missing key
const { [k1 + "z"]: v3 = 30 } = { a: 1 };
console.log(v3);

// default does NOT fire on null (only undefined)
const { [k1]: v4 = 99 } = { a: null };
console.log(v4);

// nested pattern behind a computed key
const { [k1]: { b: v5 } } = { a: { b: 5 } };
console.log(v5);

// number-valued key over an array source
const kn = 1;
const { [kn]: v6 } = ["zero", "six"];
console.log(v6);

// symbol key
const s = Symbol("s");
const src7: any = { [s]: 7 };
const { [s]: v7 } = src7;
console.log(v7);

// §14.3.3.3 — keys evaluate in field order, interleaved with binds
const order: string[] = [];
function key(n: string): string {
  order.push(n);
  return n;
}
const { [key("p")]: p, [key("q")]: q } = { p: 8, q: 9 };
console.log(p, q, order.join(","));

// let (mutable) and var forms
let { [k1]: m1 } = { a: 10 };
m1 = m1 + 1;
console.log(m1);
var { [k1]: v8 } = { a: 88 };
console.log(v8);

// for-of head pattern
const rows: any[] = [{ a: 100 }, { a: 200 }];
for (const { [k1]: rv } of rows) {
  console.log(rv);
}

// mixed static + computed fields in one pattern
const { first, [k1]: second } = { first: "f", a: "s" };
console.log(first, second);
