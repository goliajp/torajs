// §13.15.5.4 — object rest next to a computed key. The omit list is a
// comma-separated string of names, and a computed key has no name, so
// it rides to the copy kernel as the property-key VALUE it already is.
const src: any = { foo: 1, bar: 2, baz: 3 };
const name = "foo";

// declaration form
const { [name]: a, ...rest1 } = src;
console.log(a, JSON.stringify(rest1));

// assignment form
let b: any, rest2: any;
({ [name]: b, ...rest2 } = src);
console.log(b, JSON.stringify(rest2));

// parameter form
function take({ [name]: c, ...rest3 }: any) {
  console.log(c, JSON.stringify(rest3));
}
take(src);

// a spelled key and a computed one together
const { bar, [name]: d, ...rest4 } = src;
console.log(bar, d, JSON.stringify(rest4));

// the key is not a string
const nums: any = { 1: "one", 2: "two" };
for (const { [1.0]: e, ...rest5 } of [nums]) {
  console.log(e, JSON.stringify(rest5));
}

// a symbol key leaves the rest without it
const sym = Symbol("k");
const withSym: any = { [sym]: "s", x: 9 };
const { [sym]: f, ...rest6 } = withSym;
console.log(f, JSON.stringify(rest6), Object.getOwnPropertySymbols(rest6).length);

// the key expression is converted once, at its own position
let calls = 0;
const key: any = {
  toString() {
    calls = calls + 1;
    return "foo";
  },
};
const { [key]: g, ...rest7 } = src;
console.log(g, JSON.stringify(rest7), calls);
