// An element of a function array read behind a non-null assertion is
// handed to a function-typed parameter (`run(hs.at(i)!)`), with and
// without an `any` function stored in the array, the element type
// admitting `undefined` or not.
type F = (x: number) => number;
const tag: any = (x: any) => "t" + x;
const mk = (): F[] => [(x: number) => x + 1, (x: number) => x * 2, (x: number) => x - 3];

function run(f: F) {
  return f(10);
}
const more = mk();
console.log(run(more.at(1)!), run(more.at(-1)!), run(more.find((f) => f(0) === 0)!));
console.log(run(more.findLast((f) => true)!), run(more.shift()!), more.length);
more[1] = tag;
console.log(run(more.at(1)!), run(more.pop()!), run(more.at(0)!));

const ns: (F | undefined)[] = [undefined, (x: number) => x * 10];
function twice(f: F) {
  return f(f(1));
}
console.log(twice(ns.at(1)!), twice(ns.findLast((f) => f !== undefined)!));
ns[0] = tag;
console.log(twice(ns.at(0)!), twice(ns.pop()!));
