// blade 2b judgment edges: a typed program whose only any-lane
// traffic is the coercion surface — the judgment stubs most family
// arms here, and these paths must keep answering:
// - a runtime throw's error object crossing catch + member reads
// - a user object's own valueOf via OrdinaryToPrimitive
// - Array.prototype.toString (= join, in the arr arm) via template
try {
  (null as any).foo;
} catch (e) {
  console.log("caught", (e as any).name);
}
const o: any = { valueOf() { return 7; } };
console.log(o + 1);
console.log(`${[1, 2]}`);
const s = "abc";
for (const c of s) console.log(c);
