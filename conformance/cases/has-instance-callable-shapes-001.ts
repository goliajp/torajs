// §13.10.2 step 2 is GetMethod(target, @@hasInstance), and step 3
// calls what it found. tr classified the handler by reading the cell
// tag — `Tag::Closure` or the TypeError — which is the closure-cell
// probe standing in for §7.2.3 once more. A callable Proxy handler
// was refused with "Symbol.hasInstance handler is not callable"
// about a value `typeof` calls a function.
//
// The two shapes that have no closure cell each have a call protocol
// of their own, and both answers below come from the spec rather
// than from a dispatch miss: the proxy's trap decides, and a class
// object reaches §10.2.1 step 2's refusal.
function attempt(f: () => any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const yes: any = {};
yes[Symbol.hasInstance] = new Proxy(() => true, {});
console.log("proxy true   ", attempt(() => (1 as any) instanceof yes));

const no: any = {};
no[Symbol.hasInstance] = new Proxy(() => false, {});
console.log("proxy false  ", attempt(() => ({}) instanceof no));

// The handler receives the value, and `this` is the right-hand side
// — the receiver has to survive the hop through the proxy's trap.
const seen: any = {};
const probe: any = { mark: 0 };
function watcher(v: any): boolean {
  seen.v = v;
  return (this as any) === probe;
}
probe[Symbol.hasInstance] = new Proxy(watcher, {});
console.log("this + arg   ", attempt(() => (7 as any) instanceof probe));
console.log("arg seen     ", attempt(() => seen.v));

// §10.5.12 — the apply trap decides, not the target.
const trapped: any = {};
trapped[Symbol.hasInstance] = new Proxy(() => false, { apply: () => true });
console.log("apply trap   ", attempt(() => ({}) instanceof trapped));

// A class object is callable, so step 3 calls it and §10.2.1 step 2
// refuses — a TypeError, but the spec's, not the dispatcher's.
class K {}
const cls: any = {};
cls[Symbol.hasInstance] = K;
console.log("class handler", attempt(() => ({}) instanceof cls));

// Present and genuinely not callable keeps step 4's TypeError.
const bad: any = {};
Object.defineProperty(bad, Symbol.hasInstance, { value: 1 });
console.log("number handler", attempt(() => ({}) instanceof bad));

// No handler at all falls to OrdinaryHasInstance, unchanged.
class Base {}
class Derived extends Base {}
console.log("ordinary     ", attempt(() => new Derived() instanceof Base));
const plain: any = {};
plain[Symbol.hasInstance] = (v: any) => v === 3;
console.log("closure hand ", attempt(() => (3 as any) instanceof plain));
