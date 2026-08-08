// RFC 20260808-construct-channel species key 2 — the collector
// mutual-kill fix. test262 harness helpers declare `const a: any`
// inside a function while the case body declares `var a = []` under
// the SAME name; the retired pair of separately-walked collectors
// (any-shape vs lenient-array-shape) each classified the other's
// declaration as "other" and both dropped the name, so the B2 store
// arm never admitted the case's `a.constructor[Symbol.species]`
// store and the fn-expr's `this` stayed a loud reject. The merged
// props-receiver collector admits a name when EVERY declaration is a
// runtime-props shape — the two spellings no longer kill each other.
function check(x: any, y: any): boolean {
  const a: any = x; // harness-helper shape of the same name
  return a === y;
}
var callCount = 0;
var thisSeen: any;
var a = [1, 2]; // case-body shape
a.constructor = {};
a.constructor[Symbol.species] = function () {
  callCount += 1;
  thisSeen = this;
};
var r = a.concat();
console.log(callCount, typeof r, check(1, 1), check(1, 2));
console.log(thisSeen !== undefined && thisSeen !== null);
// The merged predicate also admits the DIRECT store arm on a pure
// array binding — the expando lives in the arrprops bag and the
// keyed dispatch seeds the receiver.
var seen: any;
var c = [10, 20, 30];
c.k = function () {
  seen = this;
};
c.k();
console.log(seen === c, seen.length);
