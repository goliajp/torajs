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
