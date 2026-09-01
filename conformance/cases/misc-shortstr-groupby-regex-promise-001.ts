// ShortStr values flowing through Map.groupBy keys, RegExp(any)
// construction, and Promise.resolve(any) keep value semantics; the
// churn probes assert the materialization-leak side (546-02 misc
// batch).
const xs: any = ["ab", "cd", "ab", "cd", "ab"];
const m = Map.groupBy(xs, (x: any) => x);
console.log(m.size);
console.log(JSON.stringify(m.get("ab")));
console.log(JSON.stringify(m.get("cd")));
const s: any = "ab";
const r = new RegExp(s);
console.log(r.source, r.test("xxabyy"), r.test("zz"));
Promise.resolve(s).then((v: any) => console.log("settled", v));
