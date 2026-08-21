// (`concat` / `push` / `sort` onto a split product are deliberately
// absent: they mix owned cells into a view-typed array and are a
// separate, pre-existing hole that reproduces on `let` as well —
// logged in plan-state, not this fixture's subject.)
//
// A top-level annotated `const a: string[] = s.split(" ")` is promoted
// to a data global. Its slot used to take the ANNOTATION's layout
// (Arr<Str>) while the init filled it with substring VIEWS — so every
// reader that picks a kernel by the slot's static element type decoded
// a 32-byte view as an owned string and printed its parent pointer as
// text. `let` was never promoted and always right. The slot now takes
// the representation the init actually produces; every consumer below
// must print what bun prints.

const a: string[] = "p q r".split(" ");
console.log(a);
console.log("pre", a, "post");
console.log(String(a), a + "", a.toString());
console.log(JSON.stringify(a));
console.log(a.join("-"), a.length, a[0], a.at(0), a.at(-1));
console.log(a.indexOf("q"), a.includes("r"), a.lastIndexOf("p"));
console.log(a.slice(1).join("-"), [...a].join("-"));
console.log(a.map((x: string): string => x + "!").join(""));
console.log(a.filter((x: string): boolean => x !== "q").join("-"));
for (const x of a) console.log(x);
console.log(a.flat().join("-"));

// string-typed (non-literal) separator is the same static face
const sep: string = ",";
const b: string[] = "x,y,z".split(sep);
console.log(b, b.join("+"));

// non-ASCII parent: view byte positions follow the parent's stride
const u: string[] = "世 界 ab".split(" ");
console.log(u, u.join("|"));

// nested: promoted outer, each inner is a split product
const nested: string[][] = ["a b".split(" "), "c d".split(" ")];
console.log(nested, nested.flat().join("-"));

// read from a named fn body — the reason the binding is promoted at all
function firstOf(): string {
  return a[0] + "/" + a.join("");
}
console.log(firstOf());
