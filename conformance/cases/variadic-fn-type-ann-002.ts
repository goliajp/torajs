// variadic fn-type call lanes (RFC 20260708-variadic chunk 2) — a
// `(...args: E[]) => R`-typed binding dispatches through the boxed
// dual entry with the real argc: any declared arity works, and an
// arguments.length body sees the true argument count.
function h(cb: (...args: any[]) => number): number { return cb(1, 2); }
console.log(h((a: number, b: number) => a + b));
console.log(h(function () { return arguments.length; }));
console.log(h(() => 7));
function h3(cb: (...args: any[]) => number): number { return cb(1, 2, 3, 4); }
console.log(h3(function () { return arguments.length; }));
const t: (...xs: number[]) => number = (a: number) => a * 2;
console.log(t(21));
console.log(t(21, 99));
function g(cb: (tag: string, ...rest: number[]) => number): number { return cb("x", 5, 6); }
console.log(g((tag: string, a: number, b: number) => tag.length + a + b));
