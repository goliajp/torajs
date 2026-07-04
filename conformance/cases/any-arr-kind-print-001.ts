// Any-dynamic-access RFC (20260704) S1+S2 — typed arrays crossing
// into `any` become self-describing (ARR_ELEM_KIND header field
// written at the boxing boundary) so console.log renders their raw
// slots with the right element repr instead of misreading them as
// NaN-box AnyValues (pre-fix: number[] behind any printed nothing).
// Nested `number[][]` is covered by the RFC's follow-up chunk — the
// top-level multiline shape bun uses for nested arrays is a separate
// pre-existing print-parity gap (typed direct console.log of
// number[][] prints empty today).
const t1: number[] = [1, 2, 3];
const a1: any = t1;
console.log(a1);
const t2: string[] = ["x", "y"];
const a2: any = t2;
console.log(a2);
const t3: boolean[] = [true, false];
const a3: any = t3;
console.log(a3);
const t6: number[] = [1.5, 2.5];
const a6: any = t6;
console.log(a6);
const a5: any = [1, 2, 3];
console.log(a5);
