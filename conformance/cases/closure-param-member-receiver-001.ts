// A field holds a container the same way a binding does, but the walk
// that resolves a call's receiver knew only bindings and chained array
// methods. `o.fs.push((n) => n + 1)` therefore had no receiver type at
// all, the arrow it stores kept the parameter it gets with no context,
// and `o.fs[0](3)` answered -562949953421311 while the call dispatched
// through the declared signature.

const o: { fs: ((n: number) => number)[] } = { fs: [] };
o.fs.push((n) => n + 1);
console.log("field-push", o.fs[0](3));

type Ops = { fs: ((n: number) => number)[] };
const o2: Ops = { fs: [] };
o2.fs.push((n) => n * 2);
console.log("typedecl-field-push", o2.fs[0](3));

const o3: { i: { fs: ((n: number) => number)[] } } = { i: { fs: [] } };
o3.i.fs.push((n) => n + 10);
console.log("nested-field-push", o3.i.fs[0](3));

// The rest of the element-storing family reaches a field receiver too.
const o4: { fs: ((n: number) => number)[] } = { fs: [(n) => n] };
o4.fs.unshift((n) => n + 1);
console.log("field-unshift", o4.fs[0](3));

const o5: { fs: ((n: number) => number)[] } = { fs: [(n) => n] };
o5.fs.fill((n) => n + 1);
console.log("field-fill", o5.fs[0](3));

const o6: { fs: ((n: number) => number)[] } = { fs: [(n) => n] };
console.log("field-with", o6.fs.with(0, (n) => n + 1)[0](3));

const o7: { fs: ((n: number) => number)[] } = { fs: [(n) => n] };
o7.fs.splice(1, 0, (n) => n + 1);
console.log("field-splice", o7.fs[1](3));

// A field whose type is a named function type, through the alias.
type Op = (n: number) => number;
const o8: { fs: Op[] } = { fs: [] };
o8.fs.push((n) => n + 1);
console.log("field-alias-elem", o8.fs[0](3));

// Callbacks on field receivers keep answering what they answered — the
// same receiver resolution now types their parameters instead of
// leaving them contextless.
const p: { xs: number[] } = { xs: [3, 1, 2] };
console.log("field-map", p.xs.map((x) => x * 2)[0]);
console.log("field-filter", p.xs.filter((x) => x > 1).length);
console.log("field-sort", p.xs.sort((a, b) => a - b)[0]);
console.log("field-reduce", p.xs.reduce((a, x) => a + x, 0));
console.log("field-find", p.xs.find((x) => x > 1));
console.log("field-some", p.xs.some((x) => x > 2), p.xs.every((x) => x > 0));

type Names = { xs: string[] };
const q: Names = { xs: ["b", "a"] };
console.log("typedecl-field-map", q.xs.map((s) => s + "!")[0]);

const r: { i: { xs: number[] } } = { i: { xs: [3, 1] } };
console.log("nested-field-map", r.i.xs.map((x) => x * 2)[0]);

// A field that is not a container, and a receiver that is a binding,
// both unchanged.
const s: { n: number; xs: number[] } = { n: 4, xs: [1, 2] };
console.log("plain-field", s.n, s.xs.length);

const direct: ((n: number) => number)[] = [];
direct.push((n) => n + 1);
console.log("binding-receiver", direct[0](3));
