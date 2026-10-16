// A regex literal program that reads the RegExp constructor back off
// the literal, by a spelled and by a runtime key, and constructs with it.
const re = /a/;
const C: any = (re as any).constructor;
console.log(new C("b").test("b"), re.test("a"));
const o: any = { k: "constructor" };
const D: any = (re as any)[o.k];
console.log(new D("y").test("y"));
