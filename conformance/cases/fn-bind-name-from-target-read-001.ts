// §20.2.3.2 steps 12-14 — a bound function's name comes from
// Get(Target, "name"), and anything that is not a String becomes the
// empty string. The target's intrinsic name cannot answer any of
// these: each one is a property operation the registry never sees.
const redefined: any = Object.defineProperty(function () {}, "name", {
  value: "target",
});
console.log("redefined", JSON.stringify(redefined.name));
console.log("bound", JSON.stringify((redefined.bind() as any).name));
const nonString: any = function g(): void {};
Object.defineProperty(nonString, "name", { value: 42 });
console.log("non-string", JSON.stringify((nonString.bind() as any).name));
function h(): void {}
const deleted: any = h;
delete (deleted as any).name;
console.log("deleted", JSON.stringify((deleted.bind() as any).name));
// the chain still concatenates outside-in, and the INNER bound cell's
// own name read is what the outer one asks for
const chained: any = function q(): void {};
console.log("chained", JSON.stringify((chained.bind().bind() as any).name));
// an ordinary target is unchanged
function plain(a: number, b: number): number {
  return a + b;
}
const p: any = plain;
console.log("plain", JSON.stringify((p.bind(null) as any).name), (p.bind(null, 1) as any).length);
