// An alias name is the alias only where a type is written (TS spec
// §3.10): a string literal type, a property key or a parameter name
// spelled the same names nothing.

type Id = string;
type F = "Id" | "name";
const f: F = "Id";
console.log(f);

type N = number;
let o: { N: N } = { N: 1.5 };
o.N += 1;
console.log(o.N);

let p: { readonly Id?: Id; N(x: N): N } = { Id: "k", N: (x: number) => x * 2.5 };
console.log(p.Id, p.N(2));

const g: (N: N, Id: Id) => N = (N: number, Id: string) => N + Id.length;
console.log(g(1.5, "ab"));

type K = number;

const e: K[] = [4.25];
console.log(e[0]);

// an alias inside a union is the same union the parser would spell
type Z = string;
let w: Z | boolean = true;
let w2: boolean | string = w;
w = "s";
console.log(w, w2);
type Nil = undefined;
let q: Nil | N = undefined;
q = 1.5;
let q2: N | undefined = q;
console.log(q, q2);
