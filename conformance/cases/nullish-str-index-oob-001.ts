// `??` on a typed string slot whose value can be undefined at run time
// (an index read past the end, a popped empty array, a missing argv):
// the static type says string, the value says undefined, and the
// coalescing follows the value (§13.15.2)
const a: string[] = ["x"];
const b: string[] = [];
console.log(a[0] ?? "d", a[2] ?? "d", b.pop() ?? "e", a[1] ?? a[0]);
const s: string = a[5] ?? "f";
console.log(s, typeof (a[7] ?? "g"), (a[9] ?? "h").length);
function pick(i: number): string { return a[i] ?? "none"; }
console.log(pick(0), pick(3));
