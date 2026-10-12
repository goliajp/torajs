// A function value whose face differs from the fn-typed parameter it
// reaches — an optional slot against a plain one, an `any` return
// against a `number` one — is called by what it declares: a missing
// argument is undefined, the answer is the value's own.
function caller(h: (y?: number) => number) { return h() }
function callerU(h: (y?: number) => number) { return h(undefined) }
function caller3(h: (y?: number) => number) { return h(3) }

const f = (y?: number) => (y === undefined ? 5 : y) + 1;
console.log(caller(f), callerU(f), caller3(f));

function id<T>(x: T): T { return x }
console.log(caller(id(f)), caller3(id(f)));

// the value keeps its identity through the parameter
function same(h: (y?: number) => number, g: (y?: number) => number) { return h === g }
function nameOf(h: (y?: number) => number) { return h.name }
console.log(same(f, f), nameOf(f));

// an `any` parameter takes the number the formal passes
function callerN(h: (y: number) => number) { return h(3) }
const a = (y: any) => y * 10;
console.log(callerN(a));
function mkAny(k: number) { return (y: any) => y + k }
console.log(callerN(mkAny(1)));

// a typed return against an `any` formal return
function callerA(h: () => any) { return h() }
const five = () => 5;
const word = () => "w";
console.log(callerA(five), callerA(word), typeof callerA(five));

// a declared return on the caller: the value's answer, not a cast of it
function sum(h: (y?: number) => number): number { return h() + h(10) }
console.log(sum(f));

// the same parameter reached by faces that agree keeps working
function plain(y?: number): number { return y === undefined ? 7 : y }
console.log(caller(plain), caller3(plain));
