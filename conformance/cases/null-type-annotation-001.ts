// `null` as a type annotation. Both ends of the pipeline already
// answered for the spelling — `check_type_ann` gives Type::Null and
// `ssa_lower_parse_type` gives the null-shaped pointer — but `null`
// lexes as a keyword, not an Ident, so the type-name arm could never
// produce the word. `undefined`, which lexes as an Ident, has always
// resolved: the pair split on how the lexer classifies them.
let a: null = null;
console.log(a, typeof a, a === null, a === undefined);

let b: undefined = undefined;
console.log(b, typeof b, b === undefined);

function f(x: null): null { return x; }
console.log(f(null));

const arr: null[] = [null, null, null];
console.log(arr.length, arr[0], arr[2]);

const o: { p: null; q: undefined } = { p: null, q: undefined };
console.log(o.p, o.q);

const g: (p: null) => null = f;
console.log(g(null));

// `null | T` is the same type as `T | null`, and tsc accepts both
// orders. Reading only the trailing form made the nullish side's
// POSITION load-bearing.
let n1: null | number = null;
console.log(n1);
n1 = 5;
console.log(n1 + 1);

let n2: number | null = 7;
console.log(n2);

let n3: undefined | string = undefined;
console.log(n3);
n3 = "hi";
console.log(n3.length);

let n4: null | number | undefined = null;
console.log(n4);

function h(v: null | string): string { return v === null ? "was null" : v; }
console.log(h(null), h("s"));
