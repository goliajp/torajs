// 567-04 — `let x: T;` binds nothing until its first write, and what
// it holds meanwhile is `undefined`. tr used to refuse the program
// outright (`declared Number, init has Undefined`), comparing the
// annotation against a value the program never wrote. The annotation
// that states the truth is `T | undefined`, which every later
// consumer already understands — including the per-type undefined
// sentinel an optional parameter has always used.

let n: number;
console.log(n, typeof n, n === undefined, n == null);
n = 3;
console.log(n + 1, typeof n);

let s: string;
console.log(s, typeof s);
s = "a";
console.log(s + "b", s.length);

let b: boolean;
console.log(b, typeof b);
b = true;
console.log(!b);

let a: number[];
console.log(a);
a = [1, 2];
console.log(a.length, a[0]);

class C {
  m() {
    return 1;
  }
}
let c: C;
console.log(c);
c = new C();
console.log(c.m());

let f: (a: number) => number;
console.log(f);
f = (x) => x + 1;
console.log(f(2));

let st: Set<number>;
console.log(st);
st = new Set([1]);
console.log(st.size);

// `var` shares the shape, and a multi-declarator statement gives each
// name its own annotation.
var v: number;
console.log(v);
let p: number, q: string, r: boolean;
console.log(p, q, r);

// TS's definite-assignment assertion is a claim addressed to the type
// checker with no runtime face, so the binding still starts out
// undefined.
let d!: number;
console.log(d);

// A write anywhere makes the value the written one, loop or not.
let loop: number;
for (let i = 0; i < 3; i++) {
  loop = i;
}
console.log(loop);

// Already admitting undefined, or admitting everything: unchanged.
let u: number | undefined;
let z: any;
console.log(u, z);
