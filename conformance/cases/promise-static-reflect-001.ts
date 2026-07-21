// Promise combinator statics own-property reflection (§27.2.4) —
// gOPD / hasOwnProperty / Object.hasOwn see all four combinators;
// bare cell call raises the spec step-2 TypeError; the direct
// call form keeps working.
const P: any = Promise;
for (const k of ["all", "allSettled", "any", "race"]) {
  const d = Object.getOwnPropertyDescriptor(P, k);
  console.log(k, typeof d.value, d.writable, d.enumerable, d.configurable, d.value.length, d.value.name);
  console.log(P.hasOwnProperty(k), Object.hasOwn(P, k));
}
try {
  const f = P.all;
  f([]);
} catch (e: any) {
  console.log("bare:", e instanceof TypeError);
}
// configurable leg: delete removes the own property; a
// defineProperty restore brings it back (verifyProperty protocol)
const saved = Object.getOwnPropertyDescriptor(P, "race");
delete P.race;
console.log("deleted:", P.hasOwnProperty("race"), Object.getOwnPropertyDescriptor(P, "race") === undefined, typeof P.race);
Object.defineProperty(P, "race", saved);
console.log("restored:", P.hasOwnProperty("race"), typeof P.race, P.race.name);
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then((v: any) => console.log(JSON.stringify(v)));
