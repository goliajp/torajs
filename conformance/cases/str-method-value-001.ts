// RFC 20260725-str-method-value-reify — a builtin method read as a
// VALUE off a String-typed receiver: reified interned cell with
// spec-exact call surfaces.
const s = "hello world";

// value read + typeof + identity
const m = s.slice;
console.log(typeof m);
console.log(m === s.slice);

// reflection: spec name / spec length (slice is 2; indexOf's spec
// length is 1 even though the checker sig carries 2 params)
console.log(m.name);
console.log(m.length);
const io = s.indexOf;
console.log(io.length);

// print face
console.log(m);

// .call — full args, optional-arg form, 0-arg method
console.log(m.call(s, 1, 5));
console.log(m.call(s, 6));
const up = s.toUpperCase;
console.log(up.call(s));

// .apply
console.log(m.apply(s, [0, 5]));

// .bind — receiver pre-bound, partial arg
const b = m.bind(s, 6);
console.log(b());

// inline member form
console.log(s.slice.call(s, 2, 4));

// substring-view receiver
const sub = s.slice(6);
const f2 = sub.slice;
console.log(f2.call(sub, 0, 3));

// bare call — spec this-undefined TypeError (catchable)
try {
  m(1, 5);
  console.log("no-throw");
} catch (e) {
  console.log("caught");
}
