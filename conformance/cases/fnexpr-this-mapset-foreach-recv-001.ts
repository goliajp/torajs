// Map.prototype.forEach / Set.prototype.forEach must recognise a
// promoted callback reached through a BINDING, not only an inline one.
// The four array lanes have tested for that since rotation 286; this
// pair never gained the test, so argv went down unshifted to a callee
// whose first declared parameter is the receiver — the callback read
// `this` off the first spec argument and its own first parameter off
// the second. Measured before: Map.forEach printed the KEY where the
// value belongs and bound `this` to the value, where §23.1.3.5 says an
// absent thisArg is `undefined`.
const F = function (v: any, k: any) {
  const th = this as any;
  console.log(String(v) + "/" + String(k) + (th === undefined ? "u" : "z" + th.z));
};

const m = new Map([[1, 7]]);
m.forEach(F);

const s = new Set([7]);
s.forEach(F);

// An explicit thisArg reaches the callback on both receivers.
const r: any = { z: 3 };
const m2 = new Map([[1, 7]]);
m2.forEach(F, r);
const s2 = new Set([7]);
s2.forEach(F, r);

// A this-free callback binding keeps its own answer.
const P = function (v: number, k: number) {
  console.log("p" + String(v) + "/" + String(k));
};
const m3 = new Map([[1, 7]]);
m3.forEach(P);

// The inline spelling, which already worked, is unmoved.
const s3 = new Set([7]);
s3.forEach(function (v: any) {
  const th = this as any;
  console.log(String(v) + (th === undefined ? "u" : "o"));
});
