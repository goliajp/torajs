// An omitted optional `any` field is undefined, like every other
// omitted optional field.
type O = { d?: any; l: string; e?: any };
const o: O = { l: "x" };
console.log(o.d, typeof o.d, o.l, o.e === undefined, JSON.stringify(o));
const o2: O = { d: [1], l: "y" };
console.log(o2.d.length, o2.e);
function take(q: { a?: any; b: number }) { return typeof q.a + q.b; }
console.log(take({ b: 1 }), take({ a: "s", b: 2 }));
