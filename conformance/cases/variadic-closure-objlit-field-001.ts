// An object-literal binding with a variadic closure field promotes.
// The whole binding was refused because a rest-tail field never
// static-dispatches — but the field-call lane routes exactly that
// case through the boxed dual entry, so the refusal only cost the
// binding its global home: a named-fn body reading it answered
// "unknown identifier `o`" while the same reads worked on main.
const o = { f: (...xs: number[]): number => xs.length };

function viaField(): number {
  return o.f(1, 2, 3);
}
console.log(viaField(), o.f(), o.f(9));

// a mixed binding — the fn field no longer costs the literal ones
// their slot either
const mix = { n: 1, f: (...xs: string[]): string => xs.join("-") };
function viaMixed(): string {
  return mix.f("a", "b") + mix.n;
}
console.log(viaMixed(), mix.f(), mix.n);

// the field read as a value, and written through, from a named fn
function viaValue(): number {
  const g = o.f;
  return g(1, 2, 3, 4);
}
function useIt(h: (...a: number[]) => number): number {
  return h(1, 2);
}
console.log(viaValue(), useIt(o.f));

// controls: a non-variadic field and a method field keep their lanes
const ha = (a: number): number => a + 1;
const p = { g: ha, n: 2, next(): number { return this.n + 1 } };
function viaControls(): number {
  return p.g(1) + p.next();
}
console.log(viaControls(), p.g(41), p.next());
