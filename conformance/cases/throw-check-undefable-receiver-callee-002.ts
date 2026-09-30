// The same raise through a plain function call chain and a method,
// with the try in the caller rather than in an arrow: the callee's
// guarded read (`find` miss, optional field, nullable parameter) must
// surface as the caller's catch, and the narrowed spellings must keep
// answering. Acceptance: byte-equal with bun.

class Node {
  id: number;
  next: Node | null;
  constructor(id: number, next: Node | null) {
    this.id = id;
    this.next = next;
  }
}

function nextId(n: Node): number {
  return n.next.id;
}

function nextIdChecked(n: Node): number {
  if (n.next === null) return -1;
  return n.next.id;
}

function idOf(xs: Node[], want: number): number {
  const hit = xs.find((n: Node): boolean => n.id === want);
  return hit.id;
}

function idOfOr(xs: Node[], want: number): number {
  const hit = xs.find((n: Node): boolean => n.id === want);
  return hit !== undefined ? hit.id : -1;
}

function idOfNullable(n: Node | null): number {
  return n.id;
}

function idOfNullableGuarded(n: Node | null): number {
  if (!n) return -1;
  return n.id;
}

const tail = new Node(2, null);
const head = new Node(1, tail);
const list = [head, tail];

function tryNum(label: string, f: () => number): void {
  try {
    const v = f();
    console.log(label, v);
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

tryNum("nextId head", () => nextId(head));
tryNum("nextId tail", () => nextId(tail));
tryNum("nextIdChecked tail", () => nextIdChecked(tail));
tryNum("idOf hit", () => idOf(list, 2));
tryNum("idOf miss", () => idOf(list, 9));
tryNum("idOfOr miss", () => idOfOr(list, 9));
tryNum("nullable value", () => idOfNullable(head));
tryNum("nullable null", () => idOfNullable(null));
tryNum("nullable guarded null", () => idOfNullableGuarded(null));

let caughtDirect = 0;
try {
  caughtDirect = nextId(tail);
} catch (e) {
  caughtDirect = e instanceof TypeError ? 1 : 2;
}
console.log("direct", caughtDirect);
