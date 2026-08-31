// A sentinel parked in a field or handed to a binding by assignment,
// then returned. The fall-through table's predicate had no arm for a
// field read at all, and its local binding tracking read declarations
// only — so both printed NaN at the call site while the identical
// read outside a return has answered `undefined` for some time.
const zs: number[] = [1, 2, 3];

function fromFieldLit(): number {
  const r = { v: zs[9] };
  return r.v;
}
console.log(fromFieldLit(), typeof fromFieldLit());

function fromFieldAssign(): number {
  const o = { w: 0 };
  o.w = zs[9];
  return o.w;
}
console.log(fromFieldAssign(), typeof fromFieldAssign());

function fromAssignedBinding(): number {
  let a: number = 0;
  a = zs[9];
  return a;
}
console.log(fromAssignedBinding(), typeof fromAssignedBinding());

// the declaration form, which already worked, as a control
function fromDeclaredBinding(): number {
  const m = zs[9];
  return m;
}
console.log(fromDeclaredBinding());

// assigned inside a branch, returned after it
function fromBranchAssign(flag: boolean): number {
  let b: number = 0;
  if (flag) {
    b = zs[9];
  }
  return b;
}
console.log(fromBranchAssign(true), fromBranchAssign(false));

// a field of the same name that is never handed one stays ordinary,
// and an in-range read carried out through a field stays a number
function ordinary(): number {
  const p = { v: zs[1] };
  return p.v;
}
console.log(ordinary(), typeof ordinary(), ordinary() + 1);
