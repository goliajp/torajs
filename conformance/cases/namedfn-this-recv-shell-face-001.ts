// A named fn declaration whose body says `this`, parked where the
// read-back channel honours a receiver, gets the receiver-first
// `__fwdrecv_` shell. That shell is an implementation detail: its
// NAME face and its `length` must still be the user's (§10.2.9
// SetFunctionName / §10.2.10 SetFunctionLength). Before rotation 595
// every one of these answered the synthetic mint `__fwdrecv_t` and
// counted the hidden receiver slot, so `.length` was one too many.
//
// Identity (`stored === t`) is a separate, registered residue: the
// plain and receiver-first shells are two cells, and merging them
// needs the receiver to ride the closure ABI rather than a declared
// slot. It is deliberately not asserted here.
//
// `toString` is not asserted either — bun transpiles TS before
// running it, so its answer is the reformatted source, not ours.

function t(a: any) {
  return "a=" + a + "/" + typeof (this as any);
}

// the property-store arm
const o: any = {};
o.m = t;
console.log(o.m.name, o.m.length);
console.log(o.m(1));

// the array-slot store arm
const z: any[] = [];
z[0] = t;
console.log(z[0].name, z[0].length);

// the object-literal field arm
const lit: any = { k: t };
console.log(lit.k.name, lit.k.length);
console.log(lit.k(2));

// the any-let bare-init arm
const b: any = t;
console.log(b.name, b.length);

// the HOF callback arm (thisArg present)
const src: any[] = [1];
src.forEach(t, { u: 9 });
const after: any = {};
after.m = t;
console.log(after.m.name, after.m.length);

// the declaration's own direct face is untouched
console.log(t.name, t.length);
