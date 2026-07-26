// S2.24 (RFC 20260727-dstr-assignment 刀 1) — array destructuring
// assignment to existing bindings, ES §13.15.5. The statement-form
// pattern expands at parse time into a temp + plain assignments;
// the RHS always materializes first (swap idiom depends on it).

// basics
let a = 0;
let b = 0;
[a, b] = [1, 2];
console.log(a, b); // 1 2

// swap — RHS hoists into the temp before any target writes
[a, b] = [b, a];
console.log(a, b); // 2 1

// member targets
let o = { x: 0, y: 0 };
[o.x, o.y] = [5, 6];
console.log(o.x, o.y); // 5 6

// index targets, including a computed index
let arr = [0, 0, 0];
let i = 1;
[arr[0], arr[i], arr[2]] = [7, 8, 9];
console.log(arr[0], arr[1], arr[2]); // 7 8 9

// source is an existing binding (no literal on the RHS)
let src = [10, 20];
[a, b] = src;
console.log(a, b); // 10 20
