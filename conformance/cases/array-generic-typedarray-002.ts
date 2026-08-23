// The Array iterator / mutator flavors over a TypedArray receiver:
// entries/keys/values mint without validating (their next throws
// over an OOB view instead), and the mutators land on the
// canonical-numeric-index element store.
const ta = new Uint8Array([5, 6]);
const ent: any = Array.prototype.entries;
for (const [k, v] of ent.call(ta)) console.log(k, v);
for (const k of Array.prototype.keys.call(ta)) console.log("k", k);
const tm = new Uint8Array(4);
tm[0] = 3; tm[1] = 1; tm[2] = 2;
Array.prototype.reverse.call(tm);
console.log(tm[0], tm[1], tm[2], tm[3]);
Array.prototype.fill.call(tm, 9, 1, 3);
console.log(tm[0], tm[1], tm[2], tm[3]);
const ts = new Uint8Array([3, 1, 2]);
Array.prototype.sort.call(ts);
console.log(ts[0], ts[1], ts[2]);
const tc = new Uint8Array([1, 2, 3, 4]);
Array.prototype.copyWithin.call(tc, 0, 2);
console.log(tc[0], tc[1], tc[2], tc[3]);
const rab = new ArrayBuffer(4, { maxByteLength: 8 });
const oob = new Uint8Array(rab, 0, 4);
rab.resize(2);
Array.prototype.reverse.call(oob);
Array.prototype.fill.call(oob, 1);
const it = Array.prototype.entries.call(oob);
console.log("minted");
try { it.next(); } catch (e) { console.log("next throws"); }
