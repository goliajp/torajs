// A named function stored into a property slot is called back as a
// METHOD, so its `this` is the call receiver (§13.3.6.2) — not the
// `undefined` the plain forwarder shim hardwires. The store target
// decides: these are the slots whose every read-back channel honours
// the receiver, the same admission the function-expression twin of
// this family already used for the same position.
function fn(a: any) {
  return "a=" + a + "/" + typeof (this as any);
}

const arr: any[] = [];
arr[0] = fn;
console.log(arr[0](1));

// A keyed hop further in lands in the same place.
const rows: any[][] = [[]];
rows[0][0] = fn;
console.log(rows[0][0](2));

const o: any = {};
o.m = fn;
console.log(o.m(3));

// Read out of the slot and called plainly, the receiver is gone again
// — which is what a detached read means.
const d = o.m;
console.log(d(4));

// An explicit receiver still wins over the holder.
const r: any = { z: 1 };
console.log(o.m.call(r, 5));
