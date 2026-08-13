// ES §13.5.1.2 step 5, strict half — a module is strict, so a REFUSED
// delete throws a TypeError instead of answering `false`. Sloppy twin:
// `delete-nonconfigurable-sloppy-001.cts`.

const o: any = {};
Object.defineProperty(o, "fixed", { value: 7, configurable: false });
Object.defineProperty(o, "loose", { value: 8, configurable: true });

try {
  console.log(delete o.fixed);
} catch (e: any) {
  console.log("threw", e instanceof TypeError);
}
console.log(o.fixed);

// A delete that is not refused does not throw.
console.log(delete o.loose);
console.log("loose" in o);

// Absent property: nothing to refuse.
console.log(delete o.neverThere);

const frozen: any = Object.freeze({ a: 1 });
try {
  console.log(delete frozen.a);
} catch (e: any) {
  console.log("threw", e instanceof TypeError);
}
console.log(frozen.a);
