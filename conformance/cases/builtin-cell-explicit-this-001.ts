// A reified builtin method cell carries the receiver-less TypeError
// as its boxed dual entry. `f.call(x)` never jumps into it — that
// spelling classifies through `call_target` and re-dispatches the
// carried mid against `x`. The explicit-`this` kernel behind every
// OTHER "call this with that receiver" site (`__torajs_any_call_
// with_this`, 21 call sites: every proxy trap, the ToPrimitive
// accessor walk, the promise combinators) jumped straight into the
// entry, so a builtin resolved through a proxy's `get` trap and
// called with the proxy as `this` answered "builtin method called
// without a receiver (this is undefined)" — the entry's own throw,
// about a receiver that was right there in the argument.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const forwarding = { get: (t: any, k: any) => Reflect.get(t, k) };
const pobj: any = new Proxy({ a: 1 } as any, forwarding);
console.log("trap obj String  ", attempt(() => String(pobj)));
console.log("trap obj concat  ", attempt(() => "" + pobj));
console.log("trap arr join    ", attempt(() => new Proxy([3, 1, 2] as any, forwarding).join("-")));
console.log("trap arr toString", attempt(() => String(new Proxy([3, 1, 2] as any, forwarding))));
// A DETACHED call still has no receiver, and that throw is the
// correct one — the re-route is gated on the caller actually having
// handed a `this`, so this leg must not have moved.
const detached: any = Array.prototype.join;
console.log("detached builtin ", attempt(() => detached()));
// And `.call` keeps answering exactly as it did.
console.log("dot-call still   ", attempt(() => Object.prototype.toString.call([1, 2])));
