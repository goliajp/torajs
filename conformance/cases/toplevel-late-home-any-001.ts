// A top-level binding whose initializer has no fixed type (an `any`
// read, `null`, `undefined`, `void 0`, a nullish default over an `any`
// read) or is an optional chain is read from function declarations.
// It used to stay local to the program body, so every read from a
// function threw a ReferenceError; an optional chain typed as a number
// briefly stored its box bits as the number.
function show(): void {
  console.log(anyField, nothing, missing, voided, fallback);
  console.log(firstNum, firstStr, firstObj === objs[0], fieldOf, callOf);
}

const n = 7;
const o: any = { a: "A", b: 2 };
const nums = [1, 2, 3];
const strs = ["x" + String(n), "y" + String(n)];
const objs = [{ t: "o" + String(n) }];
const anyField = (o as any).a;
const nothing = null;
const missing = undefined;
const voided = void 0;
const fallback = o.c ?? "dflt";
const firstNum = nums?.[0];
const firstStr = strs?.[1];
const firstObj = objs?.[0];
const fieldOf = objs[0]?.t;
const callOf = o.f?.();

show();
show();
console.log(strs.join(","), JSON.stringify(objs));
