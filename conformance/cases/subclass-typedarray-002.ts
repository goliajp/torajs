// TypedArray + Array species with a subclass constructor face —
// §23.2.4.1 / §23.1.3.3: the family methods construct through
// `constructor[@@species]`, inherited default getter included.
class My extends Uint8Array {}
const a = new My(4);
const s = a.subarray(1);
console.log(s instanceof My, s.length);
const m = a.map(x => x);
console.log(m instanceof My);
const f = a.filter(x => x === 0);
console.log(f instanceof My, f.length);
const sl = a.slice(1, 3);
console.log(sl instanceof My, sl.length);
class MyArr extends Array {}
const ar = new MyArr(3);
const s2 = ar.slice(1);
console.log(s2 instanceof MyArr, s2.length);
