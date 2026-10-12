// A fn-typed binding holding a function value of another face than its
// annotation calls the value by the value's own convention: an
// immutable binding takes the value's face, a mutable one may hold a
// different face after every store.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

const g: (y?: number) => number = mk2(1);
console.log(g(), g(3), g(undefined));

let m: (y?: number) => number = mk2(1);
console.log(m(), m(3));
m = mk2(2);
console.log(m(), m(3));

// a store after an exact initializer
function plain(y?: number): number { return y === undefined ? 7 : y }
let n: (y?: number) => number = plain;
console.log(n(), n(3));
n = mk2(100);
console.log(n(), n(3));

// the initializer names another binding of the value
const f = (y?: number) => (y === undefined ? 5 : y) * 2;
const h: (y?: number) => number = f;
console.log(h(), h(4), h === f);

// an `any` parameter takes the number the annotation passes
function mkAny(k: number) { return (y: any) => y + k }
const a: (y: number) => number = mkAny(1);
console.log(a(3));

// a parameter narrower than the annotation: the value reads what the
// call passes
function mkStr() { return (s: string) => s.length }
const w: (s: any) => number = mkStr();
console.log(w("abcd"), w("é"));

// the value's identity and name through the binding
console.log(g === g, g.name, m.name);
