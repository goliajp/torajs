// V3-18 wedge — TS non-null assertion `<expr>!`. Pure type-side
// (narrows Nullable<T> → T at typecheck), runtime no-op. Pre-fix
// tora's parser only knew prefix `!` (logical not); postfix `!`
// hard-rejected.
//
// Subset limitation: requires an explicit terminator after `!`
// (`;`, `,`, `)`, etc) to disambiguate from `!=` / `!==` start.
// The bare-newline ASI form (`let y = x!` followed by another
// statement on next line) does NOT yet work — write `;`
// explicitly. Most modern formatters add the semicolon anyway.

let x: string | null = "hi";
let y: string = x!;
console.log(y);                       // hi

let n: number | null = 42;
let m: number = n!;
console.log(m);                       // 42

// In expression position before `,` or `)`.
function takesNum(v: number): number { return v + 1 }
let nv: number | null = 5;
console.log(takesNum(nv!));           // 6

let arr: number[] | null = [1, 2, 3];
console.log(arr!.length);             // 3
console.log(arr!);                    // [ 1, 2, 3 ]
