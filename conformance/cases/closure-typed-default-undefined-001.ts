// L3b ① — a closure's scalar-typed default must fire on a RUNTIME
// undefined argument (§10.2.1.4): the closure direct call is a
// CallIndirect with no static callee, so the call-site pad cannot
// see the undefined and the default has to materialize in the body.
const c = (x: number = 5) => x;
const u: any = undefined;
console.log(c(u)); // 5 (was NaN: any→number coercion swallowed undefined)
console.log(c(2)); // 2
console.log(c()); // 5 (pad channel)

const s = (t: string = "d") => t;
console.log(s(u)); // d (was "undefined": any→str rendered the box)
console.log(s("z")); // z

const b = (f: boolean = true) => f;
console.log(b(u)); // true
console.log(b(false)); // false

// param-order chain: a later default reads the EARLIER narrowed local
const pair = (x: number = 1, y: number = x + 1) => x * 10 + y;
console.log(pair(u, u)); // 12
console.log(pair(3, u)); // 34
console.log(pair(3, 7)); // 37

// function-expression value takes the same lane
const fe = function (n: number = 9) {
  return n;
};
console.log(fe(u)); // 9
console.log(fe(4)); // 4
