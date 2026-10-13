// A function declared to return a number that returns a value the
// checker types `any` hands that value back as it is: the return
// annotation converts nothing at run time.
function q(x: number, f: () => any): number { return f() }
console.log(q(1, () => "s"));
const r = q(2, () => 7);
console.log(r + 1, typeof r);
const s = q(3, () => "t");
console.log(typeof s + ":" + s);

function relay(f: () => any): number { return q(0, f) }
console.log(relay(() => "u"), relay(() => 4) * 2);

function viaParam(v: any): string { return v }
console.log(typeof viaParam(5) + ":" + viaParam(5));

// a function that returns a typed value keeps its number
function twice(x: number): number { return x * 2 }
console.log(twice(21));
