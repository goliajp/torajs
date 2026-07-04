// any-method-call RFC C4+ — FnSig-into-any: a named top-level fn
// crossing into an any binding wraps in its __forward_* closure so
// the bare any-call route reaches a boxed dual entry.
function add(a: number, b: number): number {
  return a + b;
}
const f: any = add;
console.log(f(1, 2));
console.log(typeof f);
// the direct call keeps its FnSig fast path
console.log(add(10, 20));
// string params cross through ToString; heap return transfers
function greet(who: string): string {
  return "hi " + who;
}
const g: any = greet;
console.log(g("tora"));
// void return answers undefined
function shout(n: number): void {
  console.log("n=" + n);
}
const s: any = shout;
console.log(s(7));
// any-to-any alias re-calls through the same cell
const alias: any = f;
console.log(alias(3, 4));
console.log("done");
