// A function with no return annotation that returns a closure answers a
// closure: the fn-typed parameter it reaches takes the closure cell,
// and, its face differing (an `any` return against a `number` one), the
// callee clone calls it by its own convention.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function caller(h: (y?: number) => number) { return h() }
function callerU(h: (y?: number) => number) { return h(undefined) }
function caller3(h: (y?: number) => number) { return h(3) }
console.log(caller(mk2(1)), callerU(mk2(1)), caller3(mk2(1)));

function id<T>(x: T): T { return x }
console.log(caller(id(mk2(1))), caller3(id(mk2(1))));

const held = mk2(10);
console.log(caller(held), caller3(held), held(), held(1));

// a default stands in for the missing argument inside the value
function mkDefault(k: number) { return (y: number = 5) => y + k }
console.log(caller(mkDefault(1)), caller3(mkDefault(1)));

// the closure chosen by a condition
function pick(flag: boolean) { return flag ? mk2(100) : (y?: number) => (y === undefined ? -1 : -y) }
console.log(caller(pick(true)), caller(pick(false)), caller3(pick(false)));

// the same value's identity and name through the parameter
function same(h: (y?: number) => number, g: (y?: number) => number) { return h === g }
function nameOf(h: (y?: number) => number) { return h.name }
console.log(same(held, held), same(held, mk2(10)), nameOf(held));

// a slot read in an unannotated function is not a closure source: the
// top-level `inc` stays a plain function for `apply`
function first(a: number[]) { return a[0] }
function inc(x: number) { return x + 1 }
function apply(f: (x: number) => number) { return f(1) }
function other() { const inc = first([5]); return inc }
console.log(other(), apply(inc));
