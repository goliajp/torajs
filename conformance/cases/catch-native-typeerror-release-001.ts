// unbound catch consumes and releases the thrown native error
const s: any = new Set([1, 2]);
let reached = 0;
for (let i = 0; i < 3; i++) {
  try {
    s.isSubsetOf(123);
    reached = 99;
  } catch {
    reached += 1;
  }
}
console.log(reached);

// bound catch: native TypeError carries message / name / stack prefix
try {
  s.isSupersetOf("nope");
} catch (e: any) {
  console.log(e.name);
  console.log(e.message.length > 0);
  console.log(typeof e.stack);
}

// user-level throw through unbound catch still works
function boom(): number {
  throw new Error("kaboom");
}
let hits = 0;
for (let i = 0; i < 2; i++) {
  try {
    boom();
  } catch {
    hits += 1;
  }
}
console.log(hits);

// rethrow out of a bound catch after reading message (engine message
// text differs; assert the relayed prefix + non-empty tail only)
function relay(): string {
  try {
    s.isSubsetOf(1 as any);
    return "no";
  } catch (e: any) {
    const m: string = e.message;
    throw new Error("relay: " + m);
  }
}
try {
  relay();
} catch (e: any) {
  console.log(e.message.startsWith("relay: "));
  console.log(e.message.length > 10);
}
