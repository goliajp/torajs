// Substr undefined sentinel crossing the any boundary decodes to a
// real undefined (rotation 185): OOB string index via optional
// index routes through box_to_any's Substr arm.
const s = "hello";
const i = 99;
const d = s?.[i];
console.log(d);
console.log(typeof d);
console.log(d === undefined);
// in-range view stays a string across the same route
const j = 1;
const e = s?.[j];
console.log(e);
console.log(typeof e);
console.log(e === undefined);
// any-typed alias
const a: any = d;
console.log(a, typeof a, a === undefined);
// any container holding both shapes
const box: any[] = [d, e];
console.log(box[0], typeof box[0]);
console.log(box[1], typeof box[1]);
console.log(box);
