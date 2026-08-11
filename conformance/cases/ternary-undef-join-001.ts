// A ternary joining a typed-array element read against `undefined`:
// the widen boxes the F64 branch through the possibly-sentinel lane
// (`box_f64_or_undef` opens blocks), and the join's Store/Br must
// land on the box's TAIL block — writing them on the stale branch
// end orphaned the box's merge block (regalloc "ValueId not
// allocated"). This is the spread-defaults expansion shape
// (`j < src.length ? src[j] : default`) after a default has been
// materialized to `undefined`.
const arr1: number[] = [40];
console.log(1 < arr1.length ? arr1[1] : undefined); // undefined
console.log(0 < arr1.length ? arr1[0] : undefined); // 40
const c: boolean = 1 < arr1.length;
console.log(c ? arr1[1] : undefined); // undefined
const i: number = 1;
console.log(i < arr1.length ? arr1[i] : undefined); // undefined
const t: any = 1 < arr1.length ? arr1[1] : undefined;
console.log(t); // undefined
// string-element lane through the same join
const ss: string[] = ["hi"];
console.log(1 < ss.length ? ss[1] : undefined); // undefined
console.log(0 < ss.length ? ss[0] : undefined); // hi
