// §7.2.2 step 2 wants an Array exotic object, and a slot holding
// the generic immortal cell is not one. The compile-time fold
// reads only the SSA type — the shape the slot COULD hold — so a
// `find` miss answered a constant `true`.

const ass: number[][] = [];
const miss = ass.find((x) => true);
console.log(Array.isArray(miss));

const q: number[] | undefined = undefined;
console.log(Array.isArray(q));

// --- a live value still folds true
const hit = [[1], [2]].find((x) => true);
console.log(Array.isArray(hit));
const q2: number[] | undefined = [1];
console.log(Array.isArray(q2));

// --- the non-nullable spellings keep their folds
console.log(Array.isArray([1, 2]));
console.log(Array.isArray({ a: 1 }));
console.log(Array.isArray("ab"));
console.log(Array.isArray(7));
console.log(Array.isArray(undefined));
console.log(Array.isArray(null));
const anyArr: any = [1];
console.log(Array.isArray(anyArr));
const anyUndef: any = undefined;
console.log(Array.isArray(anyUndef));
