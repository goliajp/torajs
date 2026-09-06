// §23.1.3.5 / §24.2.3.6 hand `Map.prototype.forEach` and
// `Set.prototype.forEach` the call site's thisArg, or `undefined` when
// it wrote none. Which receivers admit a `this`-reading callback was
// decided by a census of BINDING names, so the receiver written in
// place fell outside it and the two spellings of one program
// disagreed: the binding ran (rotation 597) and the inline `new
// Set([7])` was an unclaimed-receiver refusal. A constructor call is
// as certainly a Set as the binding the census proves holds one, and
// the call is its only consumer.
const r: any = { z: 2 };

const F = function (v: any, k: any): void {
  const th = this as any;
  console.log(String(v) + "/" + String(k) + (th === undefined ? "u" : (th.z === 2 ? "R" : "o")));
};

// The spelling that already worked.
const s = new Set([7]);
s.forEach(F);
const m = new Map([[1, 7]]);
m.forEach(F);

// The same receiver written in place.
new Set([7]).forEach(F);
new Map([[1, 7]]).forEach(F);

// And with a thisArg, which is where the receiver actually travels.
new Set([7]).forEach(F, r);
new Map([[1, 7]]).forEach(F, r);
s.forEach(F, r);
m.forEach(F, r);

// Written in place rather than reached through a name.
new Set([7]).forEach(function (v: any, k: any): void {
  const th = this as any;
  console.log(String(v) + "/" + String(k) + (th === undefined ? "u" : "o"));
});
