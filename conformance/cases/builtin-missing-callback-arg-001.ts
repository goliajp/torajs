// §18 binds a missing argument to undefined, and each of these
// methods' own clause then says "If IsCallable(callbackfn) is false,
// throw a TypeError" — a runtime answer, so the call compiles and the
// throw is emitted with no check. Refusing it at compile time would
// be the static-subset answer to a legal program.

const xs: number[] = [1, 2, 3];

function threw(f: () => void): string {
  try {
    f();
    return "no-throw";
  } catch (e: any) {
    return e.constructor.name;
  }
}

console.log("flatMap", threw(() => { (xs as any).flatMap(); }));
console.log("map", threw(() => { (xs as any).map(); }));
console.log("filter", threw(() => { (xs as any).filter(); }));
console.log("forEach", threw(() => { (xs as any).forEach(); }));
console.log("find", threw(() => { (xs as any).find(); }));
console.log("some", threw(() => { (xs as any).some(); }));
console.log("every", threw(() => { (xs as any).every(); }));
console.log("reduce", threw(() => { (xs as any).reduce(); }));

// §23.1.3.30 / §23.1.3.34 read a missing comparator as "no
// comparator", not as an uncallable one.
const a = [3, 1, 2];
a.sort();
console.log("sort", a.join(","));
console.log("toSorted", [3, 1, 2].toSorted().join(","));

// The receiver decides, not the method name: a user object owning a
// `map` that never calls what it is handed answers normally.
const o = { map: (cb: any) => "no" };
console.log("user", (o as any).map());

// A filled slot keeps working.
console.log("filled", xs.map((n: number) => n * 2).join(","));
