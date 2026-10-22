// `in` on a literal that lacks an optional member reaches
// %Object.prototype%: a property written there, by assignment or by a
// descriptor define, is found; once deleted it is not.
type P = { a?: number; b: number };
const p: P = { b: 1 };
const q: P = { a: 0, b: 2 };
console.log("a" in p, "a" in q);
(Object.prototype as any).a = 1;
console.log("a" in p, "a" in q);
delete (Object.prototype as any).a;
console.log("a" in p);
Object.defineProperty(Object.prototype, "a", { value: 2, configurable: true });
console.log("a" in p, "a" in q);
delete (Object.prototype as any).a;
console.log("a" in p, Object.keys(p).join(","));
