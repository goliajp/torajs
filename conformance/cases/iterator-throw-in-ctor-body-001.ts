// A spread over a source the checker cannot type as a static
// iterable runs the §7.4.2 protocol at runtime — `[Symbol.iterator]`
// and `next` are user code and may throw. The lowering checked right
// after, but the throw-shape analysis did not count the spread, so a
// function whose only raise was that one had its caller's check
// elided: inside any constructor body (before or after `super`) a
// throwing `[Symbol.iterator]` unwound the body and `new C()` then
// completed as if nothing had happened. Top level always checked.
class Bad { [Symbol.iterator]() { throw new Error("iter"); } }
function z() { return arguments.length; }
class P { constructor() { } }
class R6 extends P { constructor() { super(); const b: any = new Bad(); z(1, ...b); console.log("after R6"); } }
try { new R6(); console.log("no throw R6"); } catch (e: any) { console.log("caught R6", e.message); }
class R7 extends P { constructor() { const b: any = new Bad(); const x = [...b]; super(); console.log("after R7", x); } }
try { new R7(); console.log("no throw R7"); } catch (e: any) { console.log("caught R7", e.message); }
class R9 extends P { constructor() { const b: any = new Bad(); super(...b); console.log("after R9"); } }
try { new R9(); console.log("no throw R9"); } catch (e: any) { console.log("caught R9", e.message); }
class R2 extends P { constructor() { super(1, ...(new Bad() as any)); } }
try { new R2(); console.log("no throw R2"); } catch (e: any) { console.log("caught R2", e.message); }
const it: any = { [Symbol.iterator]() { throw new Error("objit"); } };
class R3 extends P { constructor() { super(...it); } }
try { new R3(); console.log("no throw R3"); } catch (e: any) { console.log("caught R3", e.message); }
function inner() { const b: any = new Bad(); return [...b]; }
function outer() { return inner(); }
try { outer(); console.log("no throw outer"); } catch (e: any) { console.log("caught outer", e.message); }
const okIt: any = { [Symbol.iterator]() { let i = 0; return { next: () => (i < 2 ? { value: i++, done: false } : { value: undefined, done: true }) }; } };
class R4 extends P { constructor() { super(...okIt); console.log("R4 ran"); } }
new R4();
function viaAny(src: any) { return [...src]; }
try { console.log(viaAny([1, 2]), viaAny("ab")); viaAny(new Bad()); } catch (e: any) { console.log("caught viaAny", e.message); }
