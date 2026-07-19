// L3b — `Object.setPrototypeOf` over an ObjectLit receiver promotes
// to the dynobj lane (the defineProperty lower_define_receiver
// precedent): the struct lane has no proto slot, so the runtime
// core's TAG_DYNOBJ gate silently no-opped the link — tr answered
// false on `getPrototypeOf(child) === base` where bun answers true,
// on the DIRECT typed-tier call.
//
// Two faces stay out, both pre-existing on the any-receiver direct
// form too (probed): `getPrototypeOf` of a null-proto'd dynobj
// answers `{}` instead of null, and the inline print of a dynobj
// does not walk proto-chain enumerables the way bun's inspect does.

const base = { greet: 1 };
const child = Object.setPrototypeOf({ own: 2 }, base);
console.log(child.own);
console.log(Object.getPrototypeOf(child) === base);

// value-read form — the receiver must be any-held here: an ObjectLit
// ARGUMENT to the boxed dispatcher still packs as a struct cell (the
// promote lives in the direct typed-tier lowering), so that shape
// stays with the objlit-anylane promote family (RFC 20260717, L3b).
const setProto = Object.setPrototypeOf;
const getProto = Object.getPrototypeOf;
const r2: any = { own: 5 };
const c2 = setProto(r2, base);
console.log(getProto(c2) === base, c2.own);

// null proto still readable through its own keys
const nul = Object.setPrototypeOf({ a: 1 }, null);
console.log(nul.a);

// chain: literal receiver, then re-target an any-held object
const mid: any = { m: 1 };
Object.setPrototypeOf(mid, base);
console.log(Object.getPrototypeOf(mid) === base, mid.m);

// churn — the promoted literal is already the owned result (no
// pass-through inc); a stranded count or a double-drop shows here
let n = 0;
for (let i = 0; i < 2000; i++) {
  const t = Object.setPrototypeOf({ own: i }, base);
  if (t.own === i) n += 1;
}
console.log(n);
