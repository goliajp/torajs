// §13.2.5.5 ToPropertyKey — a yield as an object-literal COMPUTED
// KEY inside a generator: the key expr lives in the
// objlit_computed_keys side table, so the generator's lifted-local
// rewrite must walk it or the hoisted yield temp (`__yx_N`) never
// becomes a `this.` field read.
let obj: any;
function* g1() {
  obj = { [yield]: 42 };
}
const i1 = g1();
i1.next();
i1.next("k");
console.log("data", obj.k);
// computed method name
let om: any;
function* g2() {
  om = { [yield]() { return 7; } };
}
const i2 = g2();
i2.next();
i2.next("m");
console.log("method", om.m());
// accessor pair (the t262 accessor-name-computed-yield-expr shape)
let yieldSet: any, ob3: any;
function* g3() {
  ob3 = {
    get [yield]() { return "get yield"; },
    set [yield](param: any) { yieldSet = param; },
  };
}
const i3 = g3();
i3.next();
i3.next("first");
i3.next("second");
console.log("get", ob3.first);
ob3.second = "set yield";
console.log("set", yieldSet);
// nested generator method under a yield-computed name
let og: any;
function* g4() {
  og = { *[yield]() { yield 5; } };
}
const i4 = g4();
i4.next();
i4.next("gm");
console.log("genm", JSON.stringify(og.gm().next()));
console.log("done");
