// A declaration that reads `this` and `new.target` carries both as its
// head; neither is an argument, so a direct call's `arguments.length`
// counts only what the call wrote.
function F(this: any, a?: number) {
  const t = new.target === undefined;
  return t + ":" + arguments.length + ":" + arguments[0];
}
console.log(F(1), F(2));
function G(this: any) {
  this.k = (new.target !== undefined) + ":" + arguments.length;
}
const g1: any = new (G as any)(1, 2);
console.log(g1.k);
