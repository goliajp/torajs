// Single-aliased class import: the census renames the decl to the
// alias (427-02) so self-references, statics, and the #b brand all
// follow — the walk-time rename-in-place only followed fn self-refs.
export class K {
  #b = 1;
  v() { return 42; }
  hasB(o: any) { return #b in o; }
  static mk() { return new K(); }
}
