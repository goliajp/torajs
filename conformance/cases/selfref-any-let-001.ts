// A closure initializer that names its own `any`-annotated binding.
// ES 9.1: a closure captures the BINDING, so the box has to exist
// before the mint. The lane that does that used to admit only a
// Closure slot, which made the shape work when the closure sat inside
// something (`let a: any = [function () { ... a ... }]`) and die at
// the mint when it was written bare.
let f: any = function (n: number): number { return n <= 1 ? 1 : n * f(n - 1); };
console.log(f(5));

let g: any = function () { return typeof g; };
console.log(g());

// The binding is mutable: writing it from inside its own body is an
// ordinary PutValue, and every read goes through the one cell.
let h: any = function () { h = 9; return "ran"; };
console.log(h(), h);

// Still reached by the nested route it always was.
let a: any = [function () { return typeof a; }];
console.log(a[0]());
