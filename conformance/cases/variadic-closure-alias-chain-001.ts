// An alias of an alias makes the same argument at every hop, so the
// chain resolves rather than stopping after one. `const gc = gb`
// where `gb = ga` was a hard reject; the uniqueness argument that
// admits the first hop admits each of the rest unchanged.
const ga = (...xs: number[]): number => xs.length;
const gb = ga;
const gc = gb;
const gd = gc;

function viaChain(): number {
  return gd(1, 2, 3);
}
console.log(viaChain(), gd(), gc(4, 5), gb(6), ga(7, 8));

// a variadic face reached through a chain
const face: (...xs: number[]) => number = (a: number) => a * 2;
const fb = face;
const fc = fb;
function viaFaceChain(): number {
  return fc(21);
}
console.log(viaFaceChain(), fc(21, 99), face(3));

// refcounted rest elements down a chain
const js = (...xs: string[]): string => xs.join("-");
const jt = js;
const ju = jt;
function viaStrChain(): string {
  return ju("a", "b", "c");
}
console.log(viaStrChain(), ju(), js("z"));

// control: a non-variadic chain keeps its static dispatch
const ha = (a: number): number => a + 1;
const hb = ha;
const hc = hb;
function viaPlainChain(): number {
  return hc(1);
}
console.log(viaPlainChain(), hc(41));
