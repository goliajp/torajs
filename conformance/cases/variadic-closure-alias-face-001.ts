// An alias answers the source binding's FACE, not its closure's decl.
// A variadic face over a non-variadic closure registers rest-tail
// while the lifted decl spells one fixed parameter; the alias holds
// that value through that face, so taking the decl canon made the two
// slots disagree about arity and the alias was a K.4 reject.
const face: (...xs: number[]) => number = (a: number) => a * 2;
const fb = face;

function viaFaceAlias(): number {
  return fb(21);
}
console.log(viaFaceAlias(), face(21), fb(21, 99), fb(7));

// a face whose closure takes no parameters at all
const zero: (...xs: string[]) => string = () => "z";
const zb = zero;
function viaZero(): string {
  return zb("a", "b");
}
console.log(viaZero(), zb(), zero("q"));

// controls: an annotation that agrees with the decl, and a named type
// alias (not a spelling this resolves — falls through to the decl)
const plain: (a: number) => number = (a: number) => a * 3;
const pb = plain;
type F = (a: number) => number;
const named: F = (a: number) => a + 10;
const nb = named;
function viaControls(): number {
  return pb(4) + nb(5);
}
console.log(viaControls(), pb(1), nb(1));
