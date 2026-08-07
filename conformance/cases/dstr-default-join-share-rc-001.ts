// Rotation 326 — a let binding took a borrow-shaped join as if it
// owned it. The shares table had no Ternary / Nullish / logical-join
// arm, so a join over pure borrows (chunk 722 keeps those at zero rc
// traffic) was stored without the consumer +1 and the scope-end drop
// stole an arm source stake. The destructuring-default desugar mints
// exactly this shape: `let cls = <in-range> ? src[0] : __ClassExpr_N`
// — the class registry stake went through zero at the exit drain.
let [cls = class {}] = [];
console.log(cls.name);

// the same join shapes over live bindings must share, not steal
let a = [1, 2];
let b = [3];
let t = a.length > 0 ? a : b;
let j = a && b;
console.log(t.length, j.length);
console.log("done");
