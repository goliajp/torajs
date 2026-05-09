// V3-06 — `class C { f: C[] }` recursive Array-of-self field.
// Required two extensions on top of V3-05's nominal-class
// machinery:
//   1. is_assignable_to_resolved deep-resolves through Array
//      and Struct fields (with a cycle guard) so an
//      `Array(ClassRef("Tree"))` matches `Array(Struct(...))`.
//   2. `this.kids = []` in a constructor body picks up the
//      element type from the field's declared array layout
//      (typecheck + ssa_lower both special-case the bare
//      `[]` literal in field-assign position).

class Tree {
  v: number;
  kids: Tree[];
  constructor(v: number) { this.v = v; this.kids = []; }
}

let root = new Tree(1)
let a = new Tree(2)
let b = new Tree(3)
root.kids.push(a)
root.kids.push(b)

console.log(root.v)
console.log(root.kids.length)
console.log(root.kids[0].v)
console.log(root.kids[1].v)
