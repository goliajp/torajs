class C { method(a, b = 39) { return a + b } }
const c: any = new C();
console.log(c.method(42));
console.log(c.method(42, undefined));
console.log(new C().method(42));
