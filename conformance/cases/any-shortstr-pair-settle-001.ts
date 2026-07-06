// pair-unbox settle — borrow-shaped (tag, value) consumers reclaim the
// ShortStr-materialized temp after the call (chunk 609). Behavior must
// stay byte-equal with bun; the leak itself is covered by the AOT RSS
// probe (leak-pair-unbox.ts, mini /tmp/rc4-dump).
const a: any = "ab";
const b: any = "cd";
console.log(a < b, a > b, a + b);
const n: any = "12";
console.log(n * 2, n - 1, +n, -n);
const f: any = "3.5";
console.log(parseInt(n), parseFloat(f));
const s5: any = "5";
console.log(Number.isInteger(s5), Number.isNaN(s5), Number.isFinite(s5));
const arr: any[] = [0, 0, 0];
arr.fill("xy" as any, 0, 2);
console.log(arr[0], arr[1], arr[2]);
console.log(`${a}!`);
const long: any = "abcdefgh";
console.log(long + a, long < a);
