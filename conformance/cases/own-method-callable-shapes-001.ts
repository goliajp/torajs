// §7.2.3 IsCallable names three shapes, and every method dispatch in
// the any lane asked for only one of them: it probed the resolved
// value for a closure `(env, entry)` pair and read a miss as "not
// callable". A callable Proxy and a class object have no pair of
// their own, so both were turned away.
//
// This is the refusing half of the family — a TypeError where the
// spec says to call — and it reaches far, because OrdinaryToPrimitive
// walks `valueOf` / `toString` through this same dispatch. `({
// valueOf: new Proxy(fn, {}) }) * 1` threw about a value `typeof`
// calls a function.
//
// Both storage shapes are covered here on purpose: a literal's field
// lives in a nominal struct slot, an assigned one in the props bag,
// and the two dispatchers each carried their own copy of the probe.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const pnum: any = new Proxy(() => 42, {});
const pstr: any = new Proxy(() => "str", {});
const trapped: any = new Proxy(() => 0, {
  apply: () => 7,
});
class K {}

// Struct slot (literal) and props bag (assigned after) — same answer.
const lit: any = { valueOf: pnum };
const bag: any = {};
bag.valueOf = pnum;
console.log("lit valueOf   ", attempt(() => lit * 1));
console.log("bag valueOf   ", attempt(() => bag * 1));
console.log("lit direct    ", attempt(() => lit.valueOf()));
console.log("bag direct    ", attempt(() => bag.valueOf()));

// The hint-string walk reaches toString through the same dispatch.
const s: any = { toString: pstr };
console.log("toString      ", attempt(() => "" + s));

// §10.5.12 — the handler decides the call, not the target.
const t: any = { m: trapped };
console.log("apply trap    ", attempt(() => t.m()));

// §10.2.1 step 2 — a class object IS callable, and refusing the call
// is that step's answer, not a dispatch miss. The message differs
// from the one a non-callable value gets.
const c: any = { m: K };
console.log("class method  ", attempt(() => c.m()));

// Everything genuinely not callable keeps its TypeError.
const n: any = { m: 5 };
const u: any = { m: undefined };
console.log("number method ", attempt(() => n.m()));
console.log("undef method  ", attempt(() => u.m()));

// The ordinary closure path, one branch unchanged.
const f: any = { valueOf: () => 11, m: (x: number) => x + 1 };
console.log("closure valueOf", attempt(() => f * 1));
console.log("closure method ", attempt(() => f.m(1)));

// §7.1.1.1 still SKIPS a non-callable own entry and tries the next
// name — the probe in front of the walk is a separate question.
const skip: any = { toString: undefined, valueOf: pnum };
console.log("skip to valueOf", attempt(() => skip * 1));
