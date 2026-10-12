// A function value handed to a fn-typed parameter of a generic function
// whose face differs from the parameter's: the call is admitted, and
// every specialization calls the value by its own convention.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function mkA(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) * k }
function mkB(k: number): (y: any) => number { return (y: any) => y + k }

function pass<T>(x: T, f: (y?: number) => number): number { return f() + f(2) }
function wrap<T>(x: T, f: (y: number) => number): T[] { return [x, x].slice(0, f(1)) }

console.log(pass("a", mk2(1)), pass(3, mk2(2)), pass(true, (y?: number) => 1));
console.log(pass("b", mkA(3)), pass([1], mkA(2)), pass(null, mk2(0)));
console.log(wrap("w", mkA(1)).length, wrap(7, mkB(1)).length, wrap(8, (y: number) => y).length);
