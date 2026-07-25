// The string row of the same binding-boundary table as
// anylane-number-annotation-001: a `string` annotation on an `any`
// init unboxes (§7.1.17 ToString) rather than storing the box's bits
// into a Str slot.
//
// Function-scoped throughout: a module-level refcounted binding takes
// the K.3/K.4 global lane instead, which rejects this init shape
// loudly and has its own row to fill (L3b).

function main() {
const bare: any = "hi";
const a: string = bare;
console.log("bare ident   :", a, a.length);

const obj: any = { v: "world" };
const b: string = obj.v;
console.log("member       :", b, b.length);

const arr: any = ["p", "qq"];
const c: string = arr[1];
console.log("index        :", c, c.length);

function mkAny(): any {
  return "made";
}
const d: string = mkAny();
console.log("call return  :", d, d.length);

// the decoded string is a real Str, so it concatenates and reassigns
const e: string = bare;
console.log("concat       :", e + "!");

let f: string = obj.v;
f = f + "?";
console.log("let reassign :", f);

// a fresh owned Str per iteration: the binding drops its own at scope
// close (the loop stays flat rather than accumulating)
let total = 0;
for (let i = 0; i < 4; i++) {
  const g: string = obj.v;
  total = total + g.length;
}
console.log("loop lengths :", total);

// method receiver `this` is `any`, so `this.v` is the same crossing
const withMethod: any = {
  v: "inner",
  read() {
    const local: string = this.v;
    return local;
  },
};
console.log("this in方法   :", withMethod.read());
}
main();
