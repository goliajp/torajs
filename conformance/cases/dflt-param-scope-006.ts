// a nested function that becomes a value runs every default it has in its
// body, in parameter order (ECMA-262 §10.2.11) — ahead of the body's own
// statements, even one spelled like a default guard
function outer(k: number) {
  function inner(a: any, b: number = k) {
    if (a === undefined) a = b;
    return a + b;
  }
  return inner;
}
console.log("user guard after a default:", outer(3)(), outer(3)(1), outer(3)(1, 2));

function outer2(k: number) {
  function inner(a: number = k, b: any = a + 1, c: number = b * 2) {
    if (b === undefined) b = -1;
    return [a, b, c].join(",");
  }
  return inner;
}
const g = outer2(4);
console.log("order across lanes:", g(), g(1), g(1, 7), g(undefined, 0));

function outer3(k: string) {
  async function later(s: string = k, n: any = s.length) {
    return s + n;
  }
  return later;
}
outer3("ab")().then((v) => console.log("async value:", v));
