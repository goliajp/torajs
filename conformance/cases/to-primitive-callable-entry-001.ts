// §7.1.1.1 step 5 — "if IsCallable(method) is true". The skip probe
// that implements it held its own reading of IsCallable (a cell with
// a boxed dual entry), so an own `valueOf` that IS callable but not
// that shape was skipped, and the walk fell through to the inherited
// toString. `{valueOf: SomeClass} + 1` answered "[object Object]1"
// where the spec calls the class and lets §10.2.1 step 2 raise.
class K {
  m(): number {
    return 1;
  }
}
const Kc: any = K;
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return e && e.constructor ? e.constructor.name : "?";
  }
}
const acc: any = {
  get valueOf() {
    return Kc;
  },
};
console.log("accessor valueOf ", attempt(() => acc + 1));
const data: any = { valueOf: Kc };
console.log("data valueOf     ", attempt(() => data + 1));
const ds: any = { toString: Kc };
console.log("data toString    ", attempt(() => `${ds}`));
const tp: any = { [Symbol.toPrimitive]: Kc };
console.log("@@toPrimitive    ", attempt(() => tp + 1));

// The skip probe still does its original job: a non-callable own
// entry shadows the inherited method and the walk moves on.
const shadow: any = { toString: undefined, valueOf: () => 7 };
console.log("undefined shadow ", attempt(() => shadow + 1));
const both: any = { toString: undefined, valueOf: undefined };
console.log("both shadowed    ", attempt(() => both + 1));
const arr: any = [1, 2];
arr.toString = undefined;
console.log("arr shadow       ", attempt(() => `${arr}`));
