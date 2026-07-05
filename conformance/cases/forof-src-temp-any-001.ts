// L3b #7 — for-of src temp ownership (any path): Call-shaped any
// sources hand the loop an owned box that the after-block releases;
// Ident sources stay borrows.
const m = new Map<number, string>();
m.set(1, "a");
m.set(2, "b");
const ma: any = m;

// any method call src → owned box temp, dropped at loop exit
for (const k of ma.keys()) console.log(k);
for (const v of ma.values()) console.log(v);
console.log(ma.size);

// user fn returning any → owned box temp
function idAny(x: any): any {
  return x;
}
for (const k of idAny(ma.entries())) console.log("id", k[0], k[1]);

// Ident src stays a borrow — the binding keeps its box, loop adds
// no release (exhausted second pass runs zero iterations)
const itA: any = m.keys();
for (const k of itA) console.log("it", k);
for (const k of itA) console.log("again", k);
console.log(ma.size);

// break exit releases the owned temp
for (const k of ma.keys()) {
  console.log("b", k);
  break;
}
console.log("done");
