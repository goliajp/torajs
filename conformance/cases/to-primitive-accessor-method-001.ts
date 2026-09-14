// §7.1.1.1 OrdinaryToPrimitive step 5 — `Get(O, "valueOf")` / `Get(O, "toString")`
// is an ordinary [[Get]]: an ACCESSOR-defined method (a getter answering a function,
// test262's toPrimitiveObserver shape) is read through the getter and the function
// it answers is called with O as this. A getter answering a non-callable is skipped
// for the next name; a data-defined method still wins as before.
const calls: string[] = [];
const o: any = {
  get valueOf() {
    calls.push("get valueOf");
    return function (this: any) {
      calls.push("call valueOf");
      return 2;
    };
  },
  get toString() {
    calls.push("get toString");
    return function () {
      calls.push("call toString");
      return "s";
    };
  },
};
console.log(+o, `${o}`, o * 3, String(o));
console.log(calls.join("|"));
const p: any = {};
Object.defineProperty(p, "valueOf", {
  get() {
    return 7;
  },
});
Object.defineProperty(p, "toString", {
  get() {
    return () => "seven";
  },
});
console.log(+p, `${p}`);
const q: any = { length: o };
console.log(+q.length, Number(q.length));
console.log("done");
