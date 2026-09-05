// A `this`-using function expression written into a NESTED index
// slot. The store face decides whether such a value may be promoted
// at all, and it read only the immediate receiver: an Ident whose
// property slots live in the any world, or a `.prototype` chain. So
// `rows[0] = fn` was claimed and `rows[0][0] = fn` took the honest
// reject, even though the two land in the same place.
//
// What decides is the ROOT the keyed hops start from: each hop off
// such a binding reads a value that is itself in that world, so the
// store lands there too and comes back through the channels that
// shift argv on the promoted callee's receiver flag. The read-back
// half of that only became true in this rotation — before it, a
// nested index read seeded no receiver at all.
let kind = function () {
  return typeof (this as any);
};

const anyRoot: any = [[kind]];
anyRoot[0][0] = kind;
console.log(anyRoot[0][0]());

// Three deep — nothing is special about one extra hop.
const deep: any = [[[kind]]];
deep[0][0][0] = kind;
console.log(deep[0][0][0]());

// An inferred array root, where the slot is fn-typed rather than
// any: the read-back rides the typed indirect call's receiver gate.
const inferred = [[kind, kind]];
inferred[0][1] = kind;
console.log(inferred[0][1]());

// The receiver is that row, not merely "an object".
let isRow = function () {
  return (this as any) === rows[0];
};
const rows: any = [[isRow]];
rows[0][0] = isRow;
console.log(rows[0][0]());

// Seen through the cast a TS program has to write, the root is the
// same root.
const casted: any = [[kind]];
(casted[0] as any)[0] = kind;
console.log(casted[0][0]());

// Arguments still land after the receiver.
let pair = function (x: number, y: number) {
  return typeof (this as any) + ":" + (x + y);
};
const withArgs: any = [[pair]];
withArgs[0][0] = pair;
console.log(withArgs[0][0](1, 2));

// Detaching drops the base (§10.2.1.2).
const held = anyRoot[0][0];
console.log(held());
