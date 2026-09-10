// A spread call site into a head-less function that reads argument
// VALUES past its declared parameters. The zero-declared shape is
// fed straight off the materialized list; a declared-parameter
// shape rides its relay, whose boxed adapter unboxes the declared
// slots and enters the body with the list's true argc and argv.
// Before, `g(...xs)` on `function g(a) { arguments[1] }` was a loud
// `box_to_any element type FnSig` (test262 top cluster ②).
function g(a: number) { return [a, arguments[1], arguments.length]; }
const xs = [1, 2, 3];
console.log(g(...xs), g(0, ...xs), g(...xs, 9), g(7));
function h(a: number, b: string) { return [a, b, arguments[2], arguments.length]; }
const ys: any[] = [4, "y"];
console.log(h(1, "x", ...xs), h(...ys), h(...ys, 8, 9));
function z() { return [arguments.length, arguments[3]]; }
console.log(z(...xs), z(0, ...xs, 5), z());
function s(a: string) { return [a, arguments[1], arguments.length]; }
console.log(s(..."ab"), s("q", ..."cd"));
function d(a: number = 5, b: string = "x") { return [a, b, arguments.length]; }
const ds: any[] = [undefined, "y", 7];
console.log(d(...ds), d(...ds, 8), (d as any)(), (d as any)(undefined, undefined, 1));
const mixed: any[] = [1, "two", null, undefined, true];
function m(a: number) { return [a, arguments[1], arguments[3], arguments[4], arguments.length]; }
console.log(m(...mixed));
