// The boxed-entry census for a mutable fn-typed binding that holds a
// value of another face is kept per declaration: a same-named binding
// elsewhere, holding values of its annotation's own face, keeps its
// direct calls and its answers; a store whose face differs only where
// the calling convention cannot see it (a `never` return, a `void`
// formal) leaves the binding direct too.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

function boxed(): number {
  let f: (y?: number) => number = mk2(1);
  f = mk2(2);
  return f() + f(3);
}

function hot(n: number): number {
  let f: (y?: number) => number = (y?: number) => (y === undefined ? 1 : y) * 2;
  let s = 0;
  for (let i = 0; i < n; i++) s += f(i);
  f = (y?: number) => (y === undefined ? 1 : y) * 3;
  for (let i = 0; i < n; i++) s += f(i);
  return s;
}
console.log(boxed(), hot(1000));

// the top-level one of the same name
let f: (y?: number) => number = mk2(10);
f = mk2(20);
console.log(f(), f(1));

// a store of a never-returning value, and a store into a void slot
function fail(y?: number): never { throw new Error("no " + y) }
let g: (y?: number) => number = (y?: number) => (y === undefined ? 4 : y);
console.log(g(), g(2));
g = fail;
try { g(9) } catch (e) { console.log((e as Error).message) }

let v: (y: number) => void = (y: number) => { console.log("v", y) };
v(1);
v = (y: number) => y * 2;
v(2);
