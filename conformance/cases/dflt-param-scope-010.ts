// a nested helper whose default reads the enclosing scope evaluates it
// there (ECMA-262 §10.2.11) however it is called: directly in a loop, from
// a closure, under a block that rebinds the name, recursively, or as a value
function direct(k: number, n: number) {
  function step(acc: number, by: number = k) {
    return acc + by * 2;
  }
  let s = 0;
  for (let i = 0; i < n; i++) s = step(s);
  return s + step(0, 1);
}
console.log("direct:", direct(3, 1000));

function fromClosure(k: number) {
  function f(y: number = k) {
    return y * 2;
  }
  return f() + ":" + [1, 2].map(() => f()).join(",");
}
console.log("from a closure:", fromClosure(3));

function shadowed(k: number) {
  function f(y: number = k) {
    return y;
  }
  let r = "";
  {
    let k = 9;
    r += f() + "," + k;
  }
  for (const k of [7]) r += "," + f() + k;
  try {
    throw 1;
  } catch (k) {
    r += "," + f();
  }
  return r + "," + f();
}
console.log("rebinding frames:", shadowed(4));

function recursive(k: number) {
  function f(y: number = k, n: number = 0): number {
    return n > 2 ? y : f(undefined, n + 1);
  }
  return f();
}
console.log("recursive:", recursive(6));

function aliased(k: string) {
  function f(y: string = k) {
    return y + "!";
  }
  const g = f;
  return g() + f();
}
console.log("aliased:", aliased("a"));

function optional(n: number) {
  function f(a: number, s?: string) {
    return s === undefined ? a : a + s.length;
  }
  function g(a: number, s?: string) {
    return s === undefined ? a + n : a + s.length;
  }
  let t = 0;
  for (let i = 0; i < n; i++) t += f(i);
  return [t + f(1, "abc"), g, f] as const;
}
const [ot, og, of] = optional(10);
console.log("optional slot:", ot, og(1), og(1, "ab"), of(2), of(2, "x"));
