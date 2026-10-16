// A program whose only RegExp is minted through the constructor held as
// a value — no literal, no `new RegExp` — still builds and runs every
// RegExp it asks for: the call form and the construct form.
const R: any = RegExp;
const r = new R("a+", "g");
console.log(r.test("caat"), String(r));
console.log(R("q", "i").flags);
