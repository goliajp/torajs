// An `any` parameter accepts every argument; making it optional does
// not take that away. The optional shape is modelled as `Nullable<T>`,
// and its admit required the argument's type to equal `T` exactly — so
// with `T` = `any` the only argument it accepted was another `any`, and
// `function a(x?: any) {}` rejected `a(1)` at typecheck.

function one(x?: any): void {
  console.log(x);
}
one(1);
one(2.5);
one("s");
one(true);
one(null);
one([1, 2]);
one({ a: 1 });

function after(p: number, x?: any): void {
  console.log(p, x);
}
after(1, "s");
after(2, 3);

function between(a: number, x?: any, b?: any): void {
  console.log(a, x, b);
}
between(1, "s", 2);

// A non-`any` optional still admits exactly its own type and null.
// (An optional `number` argument's value is a separate open gap — a
// fractional one truncates and `null` reads as zero — so this only
// exercises the admit.)
function typed(x?: number): void {
  console.log(x);
}
typed(7);

function typedStr(x?: string): void {
  console.log(x);
}
typedStr("p");
typedStr(null);
