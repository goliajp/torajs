// §23.1.3.23 push / §23.1.3.34 unshift add a NEW index, so §10.1.9
// CreateDataProperty fails on a receiver whose [[Extensible]] is false
// — a TypeError, not a silent append. Six of the nine array mutators
// had no guard at all and silently mutated a frozen array:
// `Object.freeze([3,1,2]).push(9)` answered `[3,1,2,9]` while `pop`,
// `shift` and `splice` threw on the very same receiver.
//
// The predicate was read off node, not assumed: push and unshift
// throw under preventExtensions as well as seal and freeze, while pop
// throws under seal/freeze but NOT under bare preventExtensions.
//
// Two doors for an `any` receiver — the typed `any[]` lane and the
// any-method dispatch (`a: any`) — and both are exercised here.
function t(tag: string, f: (a: any) => void, mode: string) {
  const a: any[] = [3, 1, 2];
  if (mode === "freeze") Object.freeze(a);
  else if (mode === "seal") Object.seal(a);
  else Object.preventExtensions(a);
  try {
    f(a);
    console.log(mode, tag, "NO-THROW ->", JSON.stringify(a));
  } catch (e) {
    console.log(mode, tag, "threw", (e as Error).constructor.name, JSON.stringify(a));
  }
}
for (const mode of ["freeze", "seal", "ne"]) {
  t("push", (a: any) => a.push(9), mode);
  t("unshift", (a: any) => a.unshift(9), mode);
  t("push2", (a: any) => a.push(7, 8), mode);
  t("unshift2", (a: any) => a.unshift(7, 8), mode);
}
// The typed `any[]` lane, receiver spelled as the array itself.
const b: any[] = [3, 1, 2];
Object.freeze(b);
try { b.push(9); console.log("typed-lane push NO-THROW", JSON.stringify(b)); }
catch (e) { console.log("typed-lane push threw", (e as Error).constructor.name, JSON.stringify(b)); }
try { b.unshift(9); console.log("typed-lane unshift NO-THROW", JSON.stringify(b)); }
catch (e) { console.log("typed-lane unshift threw", (e as Error).constructor.name, JSON.stringify(b)); }
// An extensible receiver still grows (the guard's fast path).
const c: any[] = [1];
c.push(2);
c.unshift(0);
console.log("extensible ->", JSON.stringify(c));
