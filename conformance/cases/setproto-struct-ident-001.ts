// Rotation 203 chunk 3 closed the divergence this fixture used to
// lock: an Ident-bound object-literal receiver of
// Object.setPrototypeOf now degrades to the dynobj lane
// (dynobj_degrade introspection-receiver trigger — the
// "variable-position any-promotion" the old comment deferred to),
// so the call re-parents exactly like bun instead of throwing the
// loud no-__proto__-slot TypeError. The .expected override is gone;
// bun is the oracle again.
const base: any = { greet: () => "hi" };
const child = { own: 2 };
try {
  Object.setPrototypeOf(child, base);
  console.log("no throw");
} catch (e: any) {
  console.log("caught:", e instanceof TypeError);
}
console.log("still alive:", child.own);
