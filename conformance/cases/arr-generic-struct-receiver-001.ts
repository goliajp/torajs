// RFC 20260721-array-proto-cluster 刀 8-A G2a — a struct (Tag::Obj)
// receiver reached through the reified-cell call/apply re-dispatch
// runs the ES generic array-like arm (read family + mutators whose
// interior writes stay inside the struct's field layout); growth
// writes reject loud per the G10 struct-dynamic-props posture.

// read family over a struct array-like
let o: any = { length: 3, 0: "a", 1: "b", 2: "a" };
const indexOfFn: any = Array.prototype.indexOf;
const joinFn: any = Array.prototype.join;
const atFn: any = Array.prototype.at;
const includesFn: any = Array.prototype.includes;
console.log("indexOf:", indexOfFn.call(o, "a"), indexOfFn.call(o, "a", 1));
console.log("join:", joinFn.call(o, "-"));
console.log("at:", atFn.call(o, -1));
console.log("includes:", includesFn.call(o, "b"), includesFn.call(o, "z"));

// pop over a struct with an absent top index: Get answers undefined,
// the final Set(length) lands in the struct's own length field
let p: any = { length: 2, 3: 42 };
const popFn: any = Array.prototype.pop;
console.log("pop:", popFn.call(p));
console.log("len:", p.length, "kept:", p[3]);

// pop with a real top element
let q: any = { length: 2, 0: "x", 1: "y" };
console.log("pop2:", popFn.call(q), "len2:", q.length);

// splice species product on a zero-length struct receiver
let z: any = { length: 0 };
const spliceFn: any = Array.prototype.splice;
const removed = spliceFn.call(z);
console.log("removed is array:", Array.isArray(removed), "n:", removed.length);
console.log("z len:", z.length);

// apply flavor rides the same re-dispatch
console.log("apply:", indexOfFn.apply(o, ["b"]));
