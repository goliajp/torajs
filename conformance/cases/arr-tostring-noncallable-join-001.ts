// §23.1.3.36 step 3 — a non-callable `join` sends the call to
// %Object.prototype.toString%. The value read out of the receiver
// only names an address when it is tagged as a heap cell; every
// other tag's payload is a value, so it must not be dereferenced.
const shapes: any[] = [null, true, false, 0, 1, "join", 0n, {}, [], undefined];
for (let i = 0; i < shapes.length; i++) {
  const o: any = {};
  o.join = shapes[i];
  console.log(i, Array.prototype.toString.call(o));
}
// an own callable join still wins
console.log(Array.prototype.toString.call({ join: () => "joined" } as any));

// §21.4.4.37 step 3 is the same hand on `toISOString`
const isoShapes: any[] = [true, 0, "x", {}];
for (let i = 0; i < isoShapes.length; i++) {
  const d: any = { toISOString: isoShapes[i] };
  try {
    (Date.prototype.toJSON as any).call(d);
    console.log(i, "no throw");
  } catch (e) {
    console.log(i, "TypeError");
  }
}
console.log((Date.prototype.toJSON as any).call({ toISOString: () => "iso" }));
