// 468-01 remainder — an un-annotated top-level object literal whose
// fields are computed rather than literal. The string and boolean
// shapes were already spelled; the numeric ones were held out on the
// worry that a struct field is a different num_width key from the
// binding, so `number` could park f64 bits in an i64 field. It is the
// same key `widen_struct_fields` asks about per field, seeded by
// `width_of` of the initializer — `1 / 2` below is the witness: shape
// answers I64, and only the per-field widen makes it print 0.5.
const ints = { a: 1 + 1, b: 2 * 3, c: 7 % 4, d: 2 ** 3 };
const fracs = { half: 1 / 2, scaled: 1.5 * 2, negd: -4 };
const bigs = { x: 2n, y: 2n * 3n };
const mixed = { msg: "a" + "b", n: 3 * 3, ok: 1 < 2, big: 5n };

function g(): f64 {
  return 1.5;
}
const fromCall = { v: g() * 2 };

function reads(): void {
  console.log(ints.a, ints.b, ints.c, ints.d);
  console.log(fracs.half, fracs.scaled, fracs.negd);
  console.log(bigs.x, bigs.y);
  console.log(mixed.msg, mixed.n, mixed.ok, mixed.big);
  console.log(fromCall.v);
}

// A named-fn write of a fractional value onto an int-shaped field
// rides the alias-class widen, same as the annotated lane.
function writes(): void {
  ints.a = 2.5;
  console.log(ints.a);
}

reads();
writes();
