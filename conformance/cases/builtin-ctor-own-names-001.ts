// §10.1.11.1 — a builtin constructor's own string keys include its
// `prototype`, its static methods and its data constants, not only
// `length` / `name`. The order among them is implementation-defined,
// so the lists are compared sorted.
const show = (label: string, c: any) =>
  console.log(label, JSON.stringify(Object.getOwnPropertyNames(c).sort()));
show("Array", Array);
show("Number", Number);
show("Int8Array", Int8Array);
show("TypedArray", Object.getPrototypeOf(Int8Array));
show("String", String);
show("Map", Map);
const A: any = Array;
console.log(Object.keys(A).length, Reflect.ownKeys(A).length === Object.getOwnPropertyNames(A).length + 1);
delete A.from;
Object.defineProperty(A, "extra", { value: 1, enumerable: false, configurable: true });
Object.defineProperty(A, "of", { value: 2, configurable: true });
const names = Object.getOwnPropertyNames(A);
console.log(names.includes("from"), names.includes("extra"), names.filter((n: string) => n === "of").length);
