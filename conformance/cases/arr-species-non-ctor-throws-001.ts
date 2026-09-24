// §10.1.13 ArraySpeciesCreate step 10: a callable that is not a
// constructor is a TypeError, empty receiver included (the product of
// calling it used to be discarded silently when there was nothing to
// copy). A plain function / class species still constructs.
function t(b: any, tag: string, species: any) {
  b.constructor = {};
  b.constructor[Symbol.species] = species;
  for (const [name, run] of [
    ["map", () => b.map((x: any) => x)],
    ["filter", () => b.filter(() => true)],
    ["slice", () => b.slice()],
    ["splice", () => b.splice(0, 0)],
    ["concat", () => b.concat()],
  ] as any) {
    try {
      const r = run();
      console.log(tag, name, "ok", JSON.stringify(r));
    } catch (e: any) {
      console.log(tag, name, e.constructor === TypeError ? "TypeError" : "other");
    }
  }
}
t([], "empty-parseInt", parseInt);
t([1, 2], "two-parseInt", parseInt);
t([], "empty-arrow", (n: any) => []);
t([1, 2], "two-arrow", (n: any) => []);
t([1], "fexpr", function (this: any, n: any) {});
class K { constructor(public n: any) {} }
t([1], "class", K);
