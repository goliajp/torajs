// A container literal written at an argument position takes the type
// that position declares, the same way one written as an initializer
// takes its binding's. A bare arrow at an argument position already
// did; wrapped in a literal it got nothing, and the arrows inside kept
// the parameter they get with no context.
//
//     function take(fs: ((n: number) => number)[]): number { return fs[0](3) }
//     take([(n) => n + 1])
//     // expected Array(Function([Number], Number)),
//     //      got Array(Function([Any], Any))
//
// The object form was not loud at all — it answered its own argument
// back.

function takeArr(fs: ((n: number) => number)[]): number {
  return fs[0](3);
}
console.log("array-arg", takeArr([(n) => n + 1]));

function takeSecond(fs: ((n: number) => number)[]): number {
  return fs[1](3);
}
console.log("array-arg-second", takeSecond([(n) => n, (n) => n * 2]));

type Op = (n: number) => number;
function takeAliased(fs: Op[]): number {
  return fs[0](3);
}
console.log("array-arg-alias", takeAliased([(n) => n + 5]));

function takeObj(o: { f: (n: number) => number }): number {
  return o.f(3);
}
console.log("object-arg", takeObj({ f: (n) => n + 1 }));

type Ops = { f: (n: number) => number };
function takeNamedObj(o: Ops): number {
  return o.f(3);
}
console.log("object-arg-typedecl", takeNamedObj({ f: (n) => n * 3 }));

function takeNested(o: { i: { f: (n: number) => number } }): number {
  return o.i.f(3);
}
console.log("object-arg-nested", takeNested({ i: { f: (n) => n + 7 } }));

function takeMixed(o: { f: (a: number, b: number) => number }): number {
  return o.f(3, 7);
}
console.log("object-arg-two-params", takeMixed({ f: (a, b) => a * 100 + b }));

function takeStr(fs: ((s: string) => string)[]): string {
  return fs[0]("hi");
}
console.log("array-arg-string", takeStr([(s) => s + "!"]));

const cap = 10;
console.log("array-arg-capture", takeArr([(n) => n + cap]));

// `concat` takes containers of elements, so the literal handed to it
// takes the receiver's own type rather than its element type.
const fs: ((n: number) => number)[] = [(n) => n];
console.log("concat", fs.concat([(n) => n + 1])[1](3));

const fs2: ((n: number) => number)[] = [];
console.log("concat-onto-empty", fs2.concat([(n) => n + 2])[0](3));

// Shapes that must keep working: a bare arrow at an argument position,
// an author-annotated parameter inside the literal, a named function
// inside it, and literals with no function in them at all.
function takeCb(cb: (n: number) => number): number {
  return cb(3);
}
console.log("bare-arrow-arg", takeCb((n) => n + 1));
console.log("annotated-inside", takeArr([(n: number) => n + 1]));

function nm(n: number): number {
  return n + 1;
}
console.log("named-fn-inside", takeArr([nm]));

function takeNums(xs: number[]): number {
  return xs[0] + xs[1];
}
console.log("plain-array-arg", takeNums([4, 5]));

function takePlainObj(o: { a: number; s: string }): string {
  return o.s + o.a;
}
console.log("plain-object-arg", takePlainObj({ a: 1, s: "x" }));

const xs: number[] = [3, 1, 2];
console.log("concat-numbers", xs.concat([9])[3]);
console.log("callbacks-still", xs.map((x) => x * 2)[0], xs.filter((x) => x > 1).length);
