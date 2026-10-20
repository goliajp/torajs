// A number a promise settles with keeps its fraction when it sits
// where the program's types reach it only through `any` (ECMA-262
// §27.2.5.4): a field of a class that refers to itself, written from
// JSON text through an `any` binding, and a value of a
// `Record<string, number>` parsed from it, each handed over as `any`
// and read by `await` and by a `.then` handler.

class Node {
  n: number;
  next: Node | null;
  constructor(n: number, next: Node | null) {
    this.n = n;
    this.next = next;
  }
}

function fromAny<T>(v: any): Promise<T> {
  return Promise.resolve(v);
}

async function main() {
  const last = new Node(2, null);
  const j: any = JSON.parse("1.5");
  const head = await fromAny<Node>(new Node(j, last));
  const list = new Node(1, last);
  console.log("node", head.n + 1, head.next ? head.next.n : -1, list.n);
  const rec = await fromAny<Record<string, number>>(JSON.parse('{"a": 2.5, "b": 3}'));
  console.log("record", rec["a"] + 1, rec["b"] + 1);
  const ints: Record<string, number> = { a: 1, b: 2 };
  const own = await Promise.resolve(ints);
  console.log("record int", own["a"], own["b"]);
  const k: any = JSON.parse("7.5");
  const raw: any = new Node(k, null);
  await fromAny<Node>(raw).then((v: Node) => console.log("then node", v.n * 2));
  await fromAny<Record<string, number>>(JSON.parse('{"c": 8.5}')).then((r: Record<string, number>) =>
    console.log("then record", r["c"] * 2),
  );
}
main();
