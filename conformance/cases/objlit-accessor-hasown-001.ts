// RFC 20260714-objlit-accessor — an accessor is an own property
// (§10.4). `hasOwnProperty` / `propertyIsEnumerable` / `in` resolved the
// key against data-field names only, so they all denied a getter or
// setter property that the object plainly has. (test262's
// propertyHelper leans on exactly this trio.)

let stored: number = 5;
const g = { a: 1, get v(): number { return 2; } };
const gs = {
  b: 1,
  get w(): number { return stored; },
  set w(x: number) { stored = x; },
};
const so = { c: 1, set u(x: number) { stored = x; } };

const ga: any = g;
const gsa: any = gs;
const soa: any = so;

console.log(ga.hasOwnProperty("v"), ga.hasOwnProperty("a"), ga.hasOwnProperty("nope"));
console.log(gsa.hasOwnProperty("w"), soa.hasOwnProperty("u"));

console.log(ga.propertyIsEnumerable("v"), ga.propertyIsEnumerable("a"), ga.propertyIsEnumerable("nope"));
console.log(gsa.propertyIsEnumerable("w"), soa.propertyIsEnumerable("u"));

console.log("v" in ga, "a" in ga, "nope" in ga);
console.log("w" in gsa, "u" in soa);

console.log(Object.hasOwn(ga, "v"), Object.hasOwn(ga, "nope"));
