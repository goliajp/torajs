// S2.36 — an inline object-literal argument to an any-dispatched
// method whose body destructures a typed struct param: the argv
// packing boxes the literal as a dynobj, the boxed adapter's
// coercion kernel materializes the struct repr the body's layout
// reads (pre-fix the process died silently at the call). Array
// destr params and pre-bound struct arguments ride along.
class B {
  named({ first, last }: { first: string, last: string }): string {
    return first + " " + last
  }
  pair([a, b]: number[]): number {
    return a * 10 + b
  }
}
let b0: any = new B();
console.log(b0.named({ first: "A", last: "S" }));
console.log(b0.pair([3, 4]));
let arg = { first: "Alice", last: "Smith" };
console.log(b0.named(arg));
try {
  console.log(b0.named({ first: "x", last: "y" }), "in-try");
} catch (e) {
  console.log("caught", e);
}
console.log("after");
