// `undefined` as a first-class type annotation (the type whose only
// value is undefined). Represented like undefined values themselves
// (a null-shaped slot), so ===, typeof, params, returns, and generic
// instantiation with T = undefined all resolve.
const x: undefined = undefined;
console.log(x === undefined, typeof x);

let y: undefined;
console.log(y === undefined);

function ret(): undefined {
  return undefined;
}
console.log(ret() === undefined);

function takes(a: undefined): boolean {
  return a === undefined;
}
console.log(takes(undefined));

function sameValue<T>(a: T, b: T): boolean {
  return a === b;
}
console.log(sameValue(undefined, undefined));
console.log(sameValue("s", "t"), sameValue(1, 1));
