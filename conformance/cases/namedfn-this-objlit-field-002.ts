// A named fn declaration whose body says `this`, written as an object
// literal FIELD value, is called back as a method and must see the
// holder as `this` (§13.3.6.2).
//
// The arm existed since 401-02 but demanded the HOLDING binding carry
// an `: any` annotation. Nothing in the proof that these reads shift
// argv needs that annotation — 590-01 wrote that proof for the
// function-EXPRESSION twin at this same position and its census does
// not look at the binding at all. Four spellings were measured wrong
// against bun and were all the same omission.
//
// Not asserted here, both measured to be wrong BEFORE this change as
// well and registered separately: `o.m.call(thisArg, ...)` /
// `.apply` do not deliver their thisArg through the shell, and a
// target whose body reads `arguments` sees only its declared arity.

function t(a: any, b: any) {
  return a + "|" + b + "/" + typeof (this as any);
}

// the plain literal — no annotation anywhere
const o = { m: t };
console.log(o.m(1, 2));

// with a sibling field
const s = { n: 7, m: t };
console.log(s.m(3, 4));

// a nested literal
const nest = { inner: { m: t } };
console.log(nest.inner.m(5, 6));

// reached through a parameter
function take(x: any) {
  return x.m(7, 8);
}
console.log(take({ m: t }));

// returned out of a function
function mk() {
  return { m: t };
}
console.log(mk().m(9, 10));

// a default parameter still fills from the right place
function d(a: any, b: any = 5) {
  return a + "|" + b + "/" + typeof (this as any);
}
const dd = { m: d };
console.log(dd.m(1));

// a detached read is a plain call — `this` is undefined, not the holder
const det = o.m;
console.log(det(11, 12));

// the name face is the user's, as on every other arm
console.log(o.m.name, o.m.length);

// a declaration that never says `this` is untouched, identity included
function q(a: any, b: any) {
  return a + "|" + b;
}
const qo = { m: q };
console.log(qo.m(13, 14), qo.m.length, qo.m === q);
