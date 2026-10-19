// a narrowed value a call set to null, kept by a builtin and read back
// as a value of the type the builtin holds, or thrown and read back as
// a value of its kind
type Node = { n: number };
type Tag = { t: string };
let q: Node | null = { n: 1 };
function f(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
}

const out: Node[] = [];
if (q !== null && f()) out.push(q);
q = { n: 2 };
out.push(q);
run("pushed", () => { for (const o of out) console.log(o.n) });
run("pushed index", () => { console.log(out[1].n, out[0].n) });

const m = new Map<string, Node>();
q = { n: 3 };
if (q !== null && f()) m.set("a", q);
run("map value", () => { const v = m.get("a"); if (v !== undefined) console.log(v.n) });

// thrown, it comes back as a value of its kind
q = { n: 4 };
run("thrown", () => {
  try { if (q !== null && f()) throw q } catch (e) { console.log((e as Node).n) }
});

// values of other types are read as before
const tags: Tag[] = [{ t: "x" }, { t: "y" }];
run("other type", () => { for (const g of tags) console.log(g.t, g.t.length) });
console.log("end");
