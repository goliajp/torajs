// A Promise static read into a binding keeps reading |this| when the
// binding is called through call / apply (§27.2.4.7 step 1-2).
const orig = Promise.resolve;
orig.call(Promise, 7).then((v) => console.log("resolve call", v));
orig.apply(Promise, [3]).then((v) => console.log("resolve apply", v));
const rj = Promise.reject;
rj.call(Promise, 5).catch((e) => console.log("reject call", e));
const al = Promise.all;
al.call(Promise, [1, Promise.resolve(2)]).then((v: any) => console.log("all call", v));
const x = orig.call(Promise, 9);
x.then((v: any) => console.log("kept", v));
try {
  orig.call(undefined, 1);
} catch (e) {
  console.log("no receiver", e instanceof TypeError);
}
const m = Math.max;
console.log(m.call(null, 1, 5, 3), m.apply(null, [4, 2]), m.call(null, 7, 8) + 1);
const np = Number.parseInt;
console.log(np.call(undefined, "17"));
