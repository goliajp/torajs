// V3-18 m1.h.10 — `new Number(x)` / `new String(x)` /
// `new Boolean(x)` MVP: shortcut to the primitive value (via
// the same coercion path as the callable form). Per spec, these
// should produce wrapper Objects with [[NumberData]] / etc;
// our MVP collapses them since the dominant test262 use case is
// arithmetic / coercion, not wrapper-object identity. Full
// wrapper-object substrate is a follow-up.
console.log(new Number(5) + 10)
console.log(new Number("3.14") * 2)
console.log(new String("hi") + " there")
console.log(new Boolean(0) === false || true)
let n = new Number(42)
console.log(n + 1)
