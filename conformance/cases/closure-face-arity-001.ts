// A function value of another arity than the fn-typed parameter, whose
// face also differs in a slot or in the return: extra arguments drop,
// missing ones bind undefined, and the answer is the value's own.
function two(h: (a: number, b: number) => number) { return h(1, 2) }
function mkShort(k: number) { return (a?: number) => (a === undefined ? 50 : a) * k }
console.log(two(mkShort(10)), two((a?: number) => (a === undefined ? 50 : a) * 7));

// a shorter value returning `any`
function mkShortAny(k: number) { return (a?: number) => (a === undefined ? 50 : a) + k }
console.log(two(mkShortAny(100)));

// a longer value: the extra optional slot is undefined
function one(h: (a: number) => number) { return h(5) }
function mkLong(k: number) { return (a?: number, extra?: number) => (a === undefined ? 0 : a) + (extra === undefined ? k : extra) }
console.log(one(mkLong(100)));

// a value returning `any` with an extra `any` slot
function mkLongAny(k: number) { return (a: number, extra?: any) => extra === undefined ? a + k : extra }
console.log(one(mkLongAny(1)));

// a binding of another arity
const g: (a: number, b: number) => number = mkShortAny(3);
console.log(g(4, 9));

// the value keeps its identity through the parameter
function same(h: (a: number, b: number) => number, g: (a: number, b: number) => number) { return h === g }
const held = mkShort(2);
console.log(same(held, held), same(held, mkShort(2)));
