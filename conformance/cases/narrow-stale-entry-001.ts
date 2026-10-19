// entry-test edge cases: async callee, spread argument, a missing
// argument, a callee named like a global
type Node = { n: number };
let q: Node | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, "sync", (e as Error).constructor.name) }
  q = { n: 1 };
}

async function af(x: Node): Promise<number> { return x.n }
run("async", () => {
  if (q !== null && nullQ()) {
    af(q).then((v) => console.log("async value", v), (e) => console.log("async rejected", (e as Error).constructor.name));
    console.log("after async call");
  }
});

function sp(x: Node, y: Node): number { return x.n + y.n }
const pair: [Node, Node] = [{ n: 1 }, { n: 2 }];
run("spread", () => { if (q !== null && nullQ()) { const t: [Node, Node] = [q, q]; console.log(sp(...t)) } });
console.log(sp(...pair));

function parseFloat(x: Node): number { return x.n }
run("global-name", () => { if (q !== null && nullQ()) console.log(parseFloat(q)) });
console.log(parseFloat({ n: 5 }));

function* gen(x: Node): Generator<number> { yield x.n }
run("gen", () => { if (q !== null && nullQ()) { const g = gen(q); console.log("made"); console.log(g.next().value) } });
console.log(gen({ n: 3 }).next().value);
