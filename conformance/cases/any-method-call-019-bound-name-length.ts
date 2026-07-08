// chunk 719 — bound cell .name ("bound " prefix, nesting) + .length
// (target length minus partials, clamped at 0)
function topfn(a: number, b: number, c: number): number {
  return a + b + c;
}
const t: any = topfn;
const b1 = t.bind(null, 10);
console.log(b1.name, b1.length);
const b2 = b1.bind(null, 20);
console.log(b2.name, b2.length);
const b3 = t.bind(null, 1, 2, 3, 4);
console.log(b3.name, b3.length);
console.log(b1(2, 3), b2(5), b3());

// bound builtin method: name prefixes the interned method name,
// length subtracts from the ES-spec arity
const s: any = "hello";
const up = s.toUpperCase.bind(s);
console.log(up.name, up.length, up());
const sl = s.slice.bind(s, 1);
console.log(sl.name, sl.length, sl(3));

// zero-arg partial keeps full length
const b0 = t.bind(null);
console.log(b0.name, b0.length, b0(1, 2, 3));

// typeof / callable probes unchanged
console.log(typeof b1, typeof up);
