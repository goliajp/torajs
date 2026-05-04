// Single-param arrow without parens — `x => body`. Parser shorthand
// for `(x) => body`. Type annotation still required (inferer
// extension is separate); the parser change alone unblocks lex + parse
// error buckets.

const f = (x: number): number => x * 2;
console.log(f(5));

const g = (s: string): string => s.normalize();
console.log(g("hello"));
console.log(g("hello") === "hello");

// Arrow with block body
const h = (x: number): number => { return x + 1; };
console.log(h(10));

// Inferer on literal receivers — `[1,2,3].map(x => ...)` works
// without explicit type annotation (Array literal infers T[] from
// first element).
const a = [1, 2, 3].map(x => x * 2);
console.log(a[0], a[1], a[2]);

const b = [1, 2, 3, 4, 5].filter(x => x > 2);
console.log(b.length, b[0]);

const c = [1, 2, 3].reduce((acc, x) => acc + x, 0);
console.log(c);

const d = ["a", "b", "c"].map(s => s + "!");
console.log(d[0], d[1], d[2]);
