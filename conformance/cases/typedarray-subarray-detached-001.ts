// §23.2.3.30 step 16 -> TypedArraySpeciesCreate -> §23.2.5.1 step 11:
// InitializeTypedArrayFromArrayBuffer throws on a detached buffer.
// `mint` wrote the fields and returned, so nothing asked — and subarray
// is the one method on this slab that does not validate its receiver
// (steps 4-6 read an out-of-bounds view as a length of 0, not a throw),
// so a detached buffer had no other gate to meet. It produced a view over
// freed memory.

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + (e as any).constructor.name);
  }
}

function detached(): Uint8Array {
  const b = new ArrayBuffer(8);
  const t = new Uint8Array(b);
  (b as any).transfer();
  return t;
}

probe("subarray-detached", () => detached().subarray(0, 2));
probe("subarray-detached-noargs", () => detached().subarray());
probe("subarray-detached-start", () => detached().subarray(1));

// Control — slice already validated, and still does.
probe("slice-detached", () => detached().slice(0, 2));

// Controls — a live buffer still answers, and the tracking rule (step 12:
// a length-tracking source with no `end` stays tracking) is untouched.
const live = new Uint8Array([1, 2, 3, 4]);
console.log(Array.from(live.subarray(1, 3)).join(","));
console.log(Array.from(live.subarray(2)).join(","));
console.log(live.subarray().length);

const rb = new ArrayBuffer(4, { maxByteLength: 8 });
const track = new Uint8Array(rb);
const sub = track.subarray();
(rb as any).resize(6);
console.log("tracking stays tracking: " + String(sub.length));
