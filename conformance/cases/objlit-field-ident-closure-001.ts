// An object-literal field that NAMES a closure binding takes the typed
// lane. The shape inference never resolved such a name, so one such
// field kept the whole literal off the `__inlobj` lane: a non-variadic
// name fell back to the boxed any-promote lane, and a variadic one had
// nowhere to go at all ("unknown identifier `o`" from any named fn).
const ga = (...xs: number[]): number => xs.length;
const o = { f: ga };

function viaField(): number {
  return o.f(1, 2, 3);
}
console.log(viaField(), o.f(), o.f(9));

// the alias chain resolves here too — the field spells the source
// binding's own face, one hop or many
const gb = ga;
const gc = gb;
const chained = { f: gc, n: 4 };
function viaChain(): number {
  return chained.f(1, 2) + chained.n;
}
console.log(viaChain(), chained.f(), chained.n);

// an annotated source: the face is what the field spells, and it need
// not equal the closure's own declaration
const face: (...xs: number[]) => number = (a: number): number => a * 2;
const fo = { f: face };
function viaFace(): number {
  return fo.f(21);
}
console.log(viaFace(), fo.f(21, 99));

// a named top-level fn, and a non-variadic closure name — both were
// already reachable, both must stay so
function nf(a: number): number {
  return a + 1;
}
const hn = (a: number): number => a * 3;
const mixed = { p: nf, q: hn, s: "z", n: 7 };
function viaMixed(): string {
  return mixed.p(1) + mixed.q(2) + mixed.s + mixed.n;
}
console.log(viaMixed(), mixed.p(41), mixed.q(14), mixed.s, mixed.n);

// the field read out as a value, and passed on, from a named fn
function viaValue(): number {
  const k = o.f;
  return k(1, 2, 3, 4);
}
function useIt(h: (...a: number[]) => number): number {
  return h(1, 2);
}
console.log(viaValue(), useIt(o.f));
