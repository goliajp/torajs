// A function declared to return a scalar hands back, as is, what a
// method of a union of builtin types answers, and what an optional
// element read off a possibly missing array yields.
function f1(x: string | number): number { return x.valueOf() as any; }
function f2(x: any[] | string): number { return x.concat("c") as any; }
function f3(x: any[] | string): number { return x.at(0); }
function f4(x: any[] | string): number { return x.length > 0 ? x[0] : 0; }
console.log(f1("v"), f1(4) + 1, f2("a"), f3("s"), f3([]), f4("z"), f4(""));

function g1(x: any[] | undefined): number { return x?.[0]; }
const a: any[] = ["u"];
const b: any[] = [];
console.log(g1(a), g1(undefined), g1(b));

// a parameter left unannotated takes an `any` argument as is, its
// default notwithstanding
var values = [2, 1, 3];
function h(x = values): number { return x[0]; }
const s: any = ["s"];
console.log(h(), h([9]), h(s));
