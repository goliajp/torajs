// closures reached through `any` whose bodies build closures that build
// closures: every level reads bindings of the levels above it, and an
// argument that does not fit the declared parameter type runs the body
// under `any` at each level
const g: any = (s: string) => (t: string) => (u: string) => s.length + ":" + t.length + ":" + u.length;
console.log(g("ab")("cde")("f"));
console.log(g(7)("x")(null === null ? "yy" : ""));
console.log(g("q")(42)([1, 2]));

function f(p: string): any {
  const inner = (s: string) => {
    const y = () => s + p + s.length;
    return y;
  };
  return inner;
}
const h: any = f("P");
console.log(h("S")());
console.log(h(12)());

const rows: any = [["a", "bb"], ["ccc"]];
const out: string[] = [];
rows.forEach(function (row: string[]) {
  const tag = "#" + row.length;
  row.forEach(function (cell: string) {
    const picked = [cell, cell + cell].filter(function (v: string) {
      return v.length > tag.length;
    });
    out.push(tag + ":" + cell + ":" + picked.join("/"));
  });
});
console.log(out.join(" "));

let total = 0;
const acc: any = (k: string) => (m: string) => () => {
  total += k.length + m.length;
  return total;
};
console.log(acc("ab")("c")(), acc(5)("dd")(), total);
