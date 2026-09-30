// A constructor overwriting a field whose factory value is the
// undefined sentinel, not null: the release of the old value must leave
// the sentinel alone, the stored object keeps its other owner, and a
// second overwrite in the same constructor releases the first store.
// Acceptance: byte-equal with bun.
class Node {
  v: number;
  next: Node | null;
  constructor(v: number, next: Node | null) {
    this.v = v;
    this.next = next;
  }
}
function sum(n?: Node | null): number {
  let s = 0;
  while (n !== null && n !== undefined) {
    s = s + n.v;
    n = n.next;
  }
  return s;
}

class U {
  a: Node | undefined = undefined;
  constructor(n: Node) {
    this.a = n;
  }
}
const shared = new Node(2, new Node(3, null));
console.log("undef-old:", sum(new U(shared).a), sum(shared));

class U2 {
  a: Node | undefined = undefined;
  constructor(x: Node, y: Node) {
    this.a = x;
    this.a = y;
  }
}
const u2 = new U2(new Node(10, null), new Node(20, shared));
console.log("undef-twice:", sum(u2.a), sum(shared));

class U3 {
  a: Node | undefined;
  constructor(n: Node | undefined) {
    this.a = n;
  }
}
console.log("undef-in:", sum(new U3(undefined).a), sum(new U3(shared).a));

let churn = 0;
for (let i = 0; i < 20000; i = i + 1) {
  const u = new U(new Node(i, null));
  churn = churn + sum(u.a);
}
console.log("churn:", churn);
