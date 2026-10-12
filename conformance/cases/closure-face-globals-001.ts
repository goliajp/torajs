// A top-level fn-typed binding that a store hands a value of another
// face reaches every function as a module global, whether it was
// declared alone, in a multi-declarator list, with a pattern beside
// it, as `var`, or exported.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

let a = 1, f: (y?: number) => number = (y?: number) => (y === undefined ? 5 : y) + 1;
let [p, q] = [2, 3], f2: (y?: number) => number = (y?: number) => (y === undefined ? 5 : y) + 1;
var b = 4, f3: (y?: number) => number = (y?: number) => (y === undefined ? 5 : y) + 1;
export let c = 5, f4: (y?: number) => number = (y?: number) => (y === undefined ? 5 : y) + 1;
f = mk2(2);
f2 = mk2(3);
f3 = mk2(4);
f4 = mk2(5);

function useAll() { return [f(), f(3), f2(), f2(3), f3(), f3(3), f4(), f4(3)].join(",") }
const viaArrow = () => f() + f2(1);
console.log(a, p, q, b, c, useAll(), viaArrow());
