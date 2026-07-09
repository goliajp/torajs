// RFC 20260709-closure-global chunk 2 — an un-annotated top-level
// closure binding is visible to named-fn bodies: the slot promotes
// under the sig synthesized from the lifted arrow's anns (params
// backfilled `any`, return inferred by the preinfer pass).
const add = (a: number, b: number): number => a + b;
function useAdd(): number {
  return add(2, 3);
}
console.log(useAdd(), add(10, 4));
// bare params — preinfer backfills (any, any)
const pick = (a, b) => b;
function usePick(): number {
  return pick(1, 42);
}
console.log(usePick(), pick("x", 7));
// str params + ret
const greet = (n: string): string => "hi " + n;
function useGreet(): string {
  return greet("bob");
}
console.log(useGreet(), greet("amy"));
// capturing arrow (captures a top-level Copy binding)
let base = 100;
const bump = (x: number): number => x + base;
function useBump(): number {
  return bump(7);
}
console.log(useBump());
// self-referential assign in a loop — the caller-side sig and the
// callee must share one width class (a floated callee answers in d0;
// a stale caller sig would read the ret off x0, the env pointer)
const stepAcc = (a: number, b: number): number => a + b;
function drive(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) {
    acc = stepAcc(acc, 1);
  }
  return acc;
}
console.log(drive(1000));
// f64-floated callee ret rides the same class as the caller sig
const half = (x: number): number => x / 2 + 0.5;
function useHalf(): number {
  return half(4);
}
console.log(useHalf());
// local alias call + reflection via any stay on the fn-local lanes
const alias = add;
let av: any = add;
console.log(alias(1, 1), av.length);
console.log("done");
