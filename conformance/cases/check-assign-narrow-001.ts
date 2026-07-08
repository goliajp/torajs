// straight-line assignment narrowing (ut3): a statement-level
// assign of a non-null value narrows a Nullable binding to its
// inner type until the next compound-statement boundary or a
// possibly-null re-assign. Re-assigning null restores the declared
// union (still assignable), and a later non-null assign narrows
// again.
let b: string | null = null;
b = "x";
console.log(b.length);
let n: number | null = null;
n = 5;
console.log(n + 1);
b = null;
b = "yz";
console.log(b.length);
n = null;
n = 40;
console.log(n + 2);
