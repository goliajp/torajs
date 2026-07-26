// A binding, a field and an element all say what they hold, and only
// the declaration position was reading it. Writing an arrow into an
// already-typed place left it contextless while the call still
// dispatched through the declared signature:
//
//     let g: (n: number) => number = (n) => n;
//     g = (n) => n + 1;
//     g(3)          // SIGSEGV
//     fs[0] = cb    // -562949953421311 on read back
//     o.f = cb      // crashed the same way the rebind did

let g: (n: number) => number = (n) => n;
g = (n) => n + 1;
console.log("rebind", g(3));

let h: (n: number) => number;
h = (n) => n * 2;
console.log("assign-after-bare-decl", h(3));

const fs: ((n: number) => number)[] = [(n) => n];
fs[0] = (n) => n + 1;
console.log("index-assign", fs[0](3));

const fs2: ((n: number) => number)[] = [(n) => n];
const at = 0;
fs2[at] = (n) => n + 5;
console.log("index-assign-via-binding", fs2[0](3));

const o: { f: (n: number) => number } = { f: (n) => n };
o.f = (n) => n + 1;
console.log("field-assign", o.f(3));

const o2: { i: { f: (n: number) => number } } = { i: { f: (n) => n } };
o2.i.f = (n) => n + 1;
console.log("nested-field-assign", o2.i.f(3));

class Store {
  fs: ((n: number) => number)[] = [(n) => n];
}
const st = new Store();
st.fs[0] = (n) => n + 1;
console.log("instance-element-assign", st.fs[0](3));

// The write does not have to sit next to the declaration.
const fs3: ((n: number) => number)[] = [(n) => n];
function set(): void {
  fs3[0] = (n) => n + 7;
}
set();
console.log("assign-from-fn-body", fs3[0](3));

// Two params, other element types, and a capturing arrow.
const two: ((a: number, b: number) => number)[] = [(a, b) => a];
two[0] = (a, b) => a * 100 + b;
console.log("two-params", two[0](3, 7));

let ss: (s: string) => string = (s) => s;
ss = (s) => s + "!";
console.log("string", ss("hi"));

const cap = 10;
let gc: (n: number) => number = (n) => n;
gc = (n) => n + cap;
console.log("capturing", gc(3));

// Shapes that must keep working: an author-annotated parameter, a
// named function, an `any` target, and assignments of ordinary values.
let ga: (n: number) => number = (n) => n;
ga = (n: number) => n + 1;
console.log("annotated-param", ga(3));

function nm(n: number): number {
  return n + 1;
}
let gn: (n: number) => number = (n) => n;
gn = nm;
console.log("named-fn", gn(3));

let anyslot: any = 1;
anyslot = (n: number) => n + 1;
console.log("any-target", anyslot(3));

let n2: number = 1;
n2 = 2;
const xs: number[] = [1];
xs[0] = 5;
const plain: { a: number } = { a: 1 };
plain.a = 7;
console.log("plain-assigns", n2, xs[0], plain.a);
