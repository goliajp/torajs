// annotated module-level closure read + called from named fns
const add: (a: number, b: number) => number = (a: number, b: number) => a + b;
function useAdd(): number {
  return add(2, 3);
}
console.log(useAdd(), add(10, 4));
// str-typed params + ret
const greet: (n: string) => string = (n: string) => "hi " + n;
function useGreet(): string {
  return greet("bob");
}
console.log(useGreet(), greet("amy"));
// capturing closure as a global (captures a top-level Copy binding)
let base = 100;
const bump: (x: number) => number = (x: number) => x + base;
function useBump(): number {
  return bump(7);
}
console.log(useBump());
// reflection via any + call through local alias
let av: any = add;
console.log(av.name, av.length);
const alias = add;
console.log(alias(1, 1));
