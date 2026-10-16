// A program whose dispatch judgment falls back (the awaited value goes
// through a Promise constructor value) and that also reaches RegExp
// through its constructor value.
const v = await Promise.resolve(3);
const R: any = RegExp;
console.log(v, new R("d").test("d"));
