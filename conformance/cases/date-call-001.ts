// Date() without new — §21.4.2.1: returns the current time as a
// string; arguments evaluate (§13.3.6.2 step 4) and are then
// discarded, with no ToPrimitive ever run on them.
const log: string[] = [];
function tick(n: string): string {
  log.push(n);
  return n;
}
// @ts-ignore
console.log("t0", typeof Date());
// @ts-ignore
console.log("t1", typeof Date(1));
// @ts-ignore
console.log("t2", typeof Date(2000, tick("m"), tick("d")));
console.log("order", log.join(","));
const poisoned = {
  valueOf(): number {
    throw new Error("boom");
  },
};
// @ts-ignore
console.log("t3", typeof Date(poisoned));
// @ts-ignore
const s: string = Date();
// current-time string in the toString format (`Wed Aug 19 2026 …`);
// Date.parse round-trip is a separate (recorded) face.
console.log("shape", /^[A-Z][a-z]{2} [A-Z][a-z]{2} \d{2} \d{4} \d{2}:\d{2}:\d{2} GMT[+-]\d{4}/.test(s));
