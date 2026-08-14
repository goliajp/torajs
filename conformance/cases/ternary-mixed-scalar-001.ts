// 398-07 — a ternary whose branches are two DIFFERENT concrete
// scalars joins to Any (TS spells the union; tr approximates it as
// Any and boxes both branches), instead of the loud
// "ternary branches differ" reject.

function h(x: any): any {
  return x === undefined ? "undef" : 1;
}
console.log(h(undefined), h(3));

// every scalar pair, both orders
const b: any = true;
console.log(b ? 5 : "five");
console.log(!b ? 5 : "five");
const c: any = false;
console.log(c ? true : 0, c ? "y" : false);

// consumption faces: typeof, concat, arithmetic, annotated slot,
// nesting
const r = b ? "a" : 1;
console.log(typeof r, r + "!");
const s: string = b ? "a" : 1;
console.log(s);
console.log((c ? "x" : 2) + 1);
console.log(b ? (c ? 1 : "z") : false);

// same-type branches keep their concrete lanes
const n: number = 3 > 2 ? 1 : 0;
const t: string = 3 > 2 ? "a" : "b";
console.log(n, t);
