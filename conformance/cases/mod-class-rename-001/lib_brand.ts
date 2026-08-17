// Hidden class colliding with an entry class of the same name: the
// census mangles it, and the #x brand must move with the rename
// (same __priv_B__x string on both sides = silent cross-module
// brand confusion, recon knife-D silent-wrong #1).
class B {
  #x = 1;
  probe(o: any) { return #x in o; }
}
export function mkB() { return new B(); }
export function hasX(o: any) { return new B().probe(o); }
