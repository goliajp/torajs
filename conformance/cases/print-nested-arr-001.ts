// Nested-array console.log — typed direct path (elem-kind marked at
// the print_any entry) + all-array multi-line break shape (bun
// parity probed 2026-07-04). Mixed scalar/array levels stay
// single-line per the same probe.

// 1. typed number[][] direct — pre-fix this SIGSEGV'd (UNSET kind →
//    NaN-box walk dereferenced raw i64 slots as cell pointers).
const a: number[][] = [[1, 2], [3]];
console.log(a);

// 2. typed string[][] direct — inner levels quote per NaN-box walk.
const c: string[][] = [["x", "y"], ["z"]];
console.log(c);

// 3. three levels deep — indent grows two spaces per level.
const d: number[][][] = [[[1], [2]], [[3]]];
console.log(d);

// 4. same shapes behind any (Arr<Any> walker, kind chain from the
//    boxing boundary).
const e: any = [[1, 2], [3]];
console.log(e);

// 5. mixed scalar + array level stays single-line (probed bun form).
const f: any = [1, [2], 3];
console.log(f);

// 6. Object.entries — all-array outer breaks, mixed inner pairs stay
//    single-line.
console.log(Object.entries({ a: 1, b: 2 }));

// 7. scalar arrays keep the flat form.
const g: number[] = [1, 2, 3];
console.log(g);

console.log("done");
