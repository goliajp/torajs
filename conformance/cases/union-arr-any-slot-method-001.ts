// methods on a parameter typed as a union of array types that the
// lowering keeps as an any value
function f(xs: (string | number)[] | any[]): any { return xs.pop() }
console.log(f(["a", 1]), f([true]), f([]))
function g(xs: (string | number)[] | boolean[]): string { return xs.join("|") + ":" + xs.length + ":" + xs.includes(false as any) }
console.log(g(["a", 2]), g([false]))
function h(xs: number[] | string[]): any { return xs.indexOf(2 as any) + "/" + xs.slice(1).length }
console.log(h([1, 2, 3]), h(["x"]))
