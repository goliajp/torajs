// §25.5.2 SerializeJSONObject walks EnumerableOwnProperties. An
// accessor IS one, and `Object.keys` on the same cell listed it — but
// the struct walk resolved each layout slot by its RAW name, and the
// read face correctly refuses the synthetic accessor spelling, so the
// property vanished from the output entirely.

const o = { a: 1, get b() { return 2; }, c: 3 };
const w: any = o;
console.log(JSON.stringify(o));
console.log(JSON.stringify(w));
console.log(JSON.stringify(Object.keys(w)), JSON.stringify(Object.values(w)));

// A get/set pair is two layout slots and one own property.
const pair = { a: 1, get p() { return 5; }, set p(v: any) {}, z: 9 };
const wp: any = pair;
console.log(JSON.stringify(wp), JSON.stringify(Object.keys(wp)));

// A set-only property's [[Get]] is undefined, and §25.5.2 drops
// undefined values — so it contributes nothing, on both spellings.
const setonly = { a: 1, set s(v: any) {} };
const ws: any = setonly;
console.log(JSON.stringify(setonly), JSON.stringify(ws));

// The getter runs once and its result serializes like any value.
const nested = { a: 1, get g() { return { q: [1, 2] }; } };
const wn: any = nested;
console.log(JSON.stringify(wn));
console.log(JSON.stringify(wn, null, 2));

// A throwing getter propagates.
const thrower = { get t() { throw new TypeError("boom"); } };
const wt: any = thrower;
try {
  JSON.stringify(wt);
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}

// A class accessor is the prototype's property, so it is NOT an own
// enumerable one and must stay out.
class C {
  a = 1;
  get b() {
    return 2;
  }
}
const c: any = new C();
console.log(JSON.stringify(c), JSON.stringify(Object.keys(c)));

// A replacer function still sees every own property.
const rep = { a: 1, get b() { return 2; } };
const wr: any = rep;
console.log(JSON.stringify(wr, (k: any, v: any) => v));
