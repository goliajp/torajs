// §25.1.6.7 `ArrayBuffer.prototype.transfer` and §25.1.6.8
// `transferToFixedLength` — one body, two names, because the spec
// gives them one: the flag that separates them only decides whether
// a resizable buffer stays resizable.
//
// This is the ONLY way a program detaches a buffer. test262 reaches
// for it through `$DETACHBUFFER`, which is why 326 cases sat behind
// it — and why the harness port defines `$262` out of this method
// rather than asking for a host hook.
//
// Two orderings below are load-bearing. `ToIndex` runs BEFORE the
// detached check, so a coercion on a dead buffer runs its user code
// and only then reports the buffer. And the allocation happens
// before the detach, so a rejected length leaves the receiver alive.

const ab: any = new ArrayBuffer(8);
const view: any = new Uint8Array(ab);
view[0] = 1;
view[1] = 2;
view[7] = 9;
console.log("before", ab.byteLength, ab.detached, view.length, view[0], view[7]);

const moved: any = ab.transfer();
console.log("after", ab.byteLength, ab.detached, moved.byteLength, moved.detached);
const mview: any = new Uint8Array(moved);
console.log("bytes", mview[0], mview[1], mview[7]);

// Every view over the transferred-from buffer learns at once: they
// all hold the same cell, and §10.4.5 answers undefined out of range
// rather than throwing.
console.log("view-after", view.length, String(view[0]));

// Growing reads the fresh buffer's zeroed tail; shrinking drops the
// bytes past the new end.
const g0: any = new ArrayBuffer(4);
new Uint8Array(g0)[0] = 5;
const grown: any = g0.transfer(8);
console.log("grow", grown.byteLength, new Uint8Array(grown)[0], new Uint8Array(grown)[7]);

const s0: any = new ArrayBuffer(4);
const s0v: any = new Uint8Array(s0);
s0v[0] = 6;
s0v[3] = 7;
const shrunk: any = s0.transfer(2);
console.log("shrink", shrunk.byteLength, new Uint8Array(shrunk)[0], new Uint8Array(shrunk).length);

// An explicit `undefined` length is the same as an absent one
// (§25.1.6.7 step 3 asks whether it IS undefined, not whether it was
// passed).
const u0: any = new ArrayBuffer(3);
console.log("undef-len", u0.transfer(undefined).byteLength);

// transfer keeps resizability; transferToFixedLength drops it.
const r0: any = new ArrayBuffer(4, { maxByteLength: 16 });
const rt: any = r0.transfer();
console.log("preserve", rt.resizable, rt.maxByteLength, rt.byteLength);

const r1: any = new ArrayBuffer(4, { maxByteLength: 16 });
const ft: any = r1.transferToFixedLength();
console.log("fixed", ft.resizable, ft.byteLength);

// A resizable transfer target still refuses a length past its max.
const r2: any = new ArrayBuffer(4, { maxByteLength: 8 });
try {
  r2.transfer(99);
  console.log("unreachable");
} catch (e: any) {
  console.log("over-max", e.constructor.name, r2.detached);
}

// A detached receiver refuses.
const d0: any = new ArrayBuffer(2);
d0.transfer();
try {
  d0.transfer();
  console.log("unreachable");
} catch (e: any) {
  console.log("twice", e.constructor.name);
}

// A zero-length buffer transfers to a zero-length buffer, and is
// still not detached.
const z: any = new ArrayBuffer(0);
const zt: any = z.transfer();
console.log("zero", zt.byteLength, zt.detached, z.detached);

// The methods read as values, and slice on the fresh buffer works.
console.log("reads", typeof moved.transfer, typeof moved.transferToFixedLength);
console.log("slice", new Uint8Array(moved.slice(0, 2)).length);
