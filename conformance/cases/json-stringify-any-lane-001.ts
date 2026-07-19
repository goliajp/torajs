// RFC 20260719-ns-static-value-reify B3b — JSON.stringify over an
// any-lane value: the runtime twin of the typed tier walk (same
// JsonBuilder output path). Covers §25.5.2 undefined/callable
// three-way split, non-finite numbers, Date toJSON, nesting,
// escaping and key order.
const a: any = { x: 1, y: "s", z: true, n: null };
console.log(JSON.stringify(a));
const b: any = [1, "two", false, null];
console.log(JSON.stringify(b));
const c: any = { nested: { deep: [1, 2] } };
console.log(JSON.stringify(c));
const d: any = 42;
console.log(JSON.stringify(d));
const e: any = "hi";
console.log(JSON.stringify(e));
const f: any = { u: undefined, keep: 1 };
console.log(JSON.stringify(f));
const g: any = [undefined, 1];
console.log(JSON.stringify(g));
const h: any = { nan: 0 / 0, inf: 1 / 0 };
console.log(JSON.stringify(h));
const i: any = { q: "he said \"hi\"" };
console.log(JSON.stringify(i));
const j: any = {};
console.log(JSON.stringify(j));

const a2: any = undefined;
console.log(JSON.stringify(a2));
const b2: any = { f: () => 1, keep: 2 };
console.log(JSON.stringify(b2));
const c2: any = [1.5, -0.25, 1e21];
console.log(JSON.stringify(c2));
const d2: any = { d: new Date(0) };
console.log(JSON.stringify(d2));
const e2: any = null;
console.log(JSON.stringify(e2));
const f2: any = [[1, [2, [3]]]];
console.log(JSON.stringify(f2));
const g2: any = { "wéird kéy": "üñí" };
console.log(JSON.stringify(g2));

