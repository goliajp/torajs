// 565-03 — the object-literal twin of 564-01. `({ [k]() {} })[k].name`
// answered `"__computed_0__"`: the parser's sentinel for a key it
// cannot spell statically, handed to §8.4.5 NamedEvaluation as if it
// were a name the user wrote, and from there into the fn-name
// registry. A computed key gives no syntactic name at all — §10.2.9
// names the member after its RUNTIME key instead, which only the
// definition point knows.
//
// The value shape is what differs from the class twin: an
// object-literal method / arrow / anonymous function expression is an
// ordinary compiler-minted closure, so the name goes where
// SetFunctionName says it goes — an own `name` property
// {writable: false, enumerable: false, configurable: true}.
//
// The inspect face answers differently again: bun reads the SOURCE,
// where a computed member has no name, and prints `[Function]`.
const k = "c1";
const sD = Symbol("d");
const sNo = Symbol();
function named() { return 9 }

const o: any = {
  plain() { return 1 },
  [k]() { return 2 },
  [42]() { return 3 },
  [sD]() { return 4 },
  [sNo]() { return 5 },
  [k + "arrow"]: () => 6,
  [k + "anon"]: function () { return 7 },
  [k + "self"]: function inner() { return 8 },
  [k + "ref"]: named,
};

console.log(JSON.stringify(o.plain.name), JSON.stringify(o[k].name));
console.log(JSON.stringify(o[42].name), JSON.stringify(o[sD].name), JSON.stringify(o[sNo].name));
console.log(JSON.stringify(o[k + "arrow"].name), JSON.stringify(o[k + "anon"].name));
// §8.4.5 renames ANONYMOUS definitions only — these two keep theirs
console.log(JSON.stringify(o[k + "self"].name), JSON.stringify(o[k + "ref"].name));

// the inspect face: named in the source vs named at runtime
console.log(o.plain, o[k], o[k + "arrow"], o[k + "anon"], o[k + "self"]);

// §10.2.9's attribute set, and the virtual key list stays a set
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o[k], "name")));
console.log(JSON.stringify(Object.getOwnPropertyNames(o[k])));
console.log(JSON.stringify(Object.keys(o[k])));

// the members still work, and identity is one cell per field
console.log(o.plain(), o[k](), o[42](), o[sD](), o[sNo]());
console.log(o[k + "arrow"](), o[k + "anon"](), o[k + "self"](), o[k + "ref"]());
console.log(o[k] === o[k], JSON.stringify(Object.keys(o)));

// a fresh key per evaluation gets a fresh name
for (let i = 0; i < 3; i++) {
  const q: any = { ["m" + i]() { return i } };
  console.log(JSON.stringify(q["m" + i].name), q["m" + i]());
}
