// The annotated spelling of an alias must agree with the bare one.
// `const gb: (...xs: number[]) => number = ga` never promoted — the
// arm admitting a variadic annotation wanted a lifted arrow on the
// right — so a named-fn body reading it answered "unknown function".
// An alias carries the same env cell the arrow itself mints.
const ga = (...xs: number[]): number => xs.length;
const gb: (...xs: number[]) => number = ga;

function viaAnnAlias(): number {
  return gb(1, 2, 3);
}
console.log(viaAnnAlias(), gb(), gb(4, 5), ga(9));

// an annotated alias of a variadic FACE over a non-variadic closure
const face: (...xs: number[]) => number = (a: number) => a * 2;
const fb: (...xs: number[]) => number = face;
function viaAnnFace(): number {
  return fb(21);
}
console.log(viaAnnFace(), fb(21, 99), face(3));

// refcounted rest elements through the annotated alias
const js = (...xs: string[]): string => xs.join("-");
const jt: (...xs: string[]) => string = js;
function viaAnnStr(): string {
  return jt("a", "b", "c");
}
console.log(viaAnnStr(), jt(), js("z"));
