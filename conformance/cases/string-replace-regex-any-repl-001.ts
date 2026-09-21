// §22.1.3.19 step 5 sets `functionalReplace` from
// IsCallable(replaceValue) — a RUN-time question. The typed regex
// lane answered it statically, admitting only a Str or a Closure, so
// a replacement that arrived in an `any` slot was refused at
// lowering: "repl must be Str or Closure, got Any".
//
// That slot is not rare. `var re = /b/` leaves an `any` binding once
// the declaration is split from its assignment, and so does every
// parameter written `any` and every function return that was not
// annotated.
//
// The literal-needle half of this has had its kernel since rotation
// 446; the regex half was the missing twin. It keeps the call's
// `Type::Str`, so the typed lane does not shrink to make room.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const fn: any = (m: string) => "<" + m + ">";
const proxied: any = new Proxy((m: string) => "[" + m + "]", {});
const text: any = "Y";
const num: any = 5;
const obj: any = {};

console.log("any fn       ", attempt(() => "abc".replace(/b/, fn)));
console.log("any proxy    ", attempt(() => "abc".replace(/b/, proxied)));
console.log("any string   ", attempt(() => "abc".replace(/b/, text)));
console.log("any number   ", attempt(() => "abc".replace(/b/, num)));
console.log("any object   ", attempt(() => "abc".replace(/b/, obj)));
console.log("replaceAll   ", attempt(() => "abcabc".replaceAll(/b/g, fn)));
console.log("replaceAll px", attempt(() => "abcabc".replaceAll(/b/g, proxied)));
console.log("replaceAll st", attempt(() => "abcabc".replaceAll(/b/g, text)));

// The capture count comes off the RegExp cell, not from a declared
// callback shape — which is the whole reason the static lane could
// not serve an unknown callable.
const caps: any = (m: string, a: string, b: string) => a + "|" + b;
console.log("captures     ", attempt(() => "a1b2".replace(/(\d)b(\d)/, caps)));
// $-patterns still belong to the string leg.
console.log("dollar amp   ", attempt(() => "abc".replace(/b/, "[$&]" as any)));

// §22.1.5 — replaceAll on a non-global RegExp is still a TypeError.
console.log("non-global   ", attempt(() => "abc".replaceAll(/b/, text)));

// A throwing replacement propagates rather than answering a string.
const boom: any = () => {
  throw new RangeError("x");
};
console.log("cb throws    ", attempt(() => "abc".replace(/b/, boom)));

// The two typed legs, one branch unchanged.
console.log("typed str    ", attempt(() => "abc".replace(/b/, "Z")));
console.log("typed fn     ", attempt(() => "abc".replace(/b/, (m: string) => m + m)));
