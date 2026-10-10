// a method writes an own "1.5" holding an object through an any[]
// view of a number[]; the typed read's ToNumber runs its valueOf,
// which raises
class Box {
  put(ys: any[], k: number) {
    ys[k] = {
      valueOf() {
        throw new Error("any[] view");
      },
    };
  }
}
const a: number[] = [1, 2];
new Box().put(a as any, parseFloat("1.5"));
function g(xs: number[], k: number): number {
  return xs[k] * 2;
}
try {
  console.log(g(a, parseFloat("1.5")));
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(g(a, 1));
console.log("end");
