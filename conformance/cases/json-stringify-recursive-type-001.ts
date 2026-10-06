// §25.5.2 SerializeJSONObject / SerializeJSONArray recurse on the
// value, not on its type: a type that names itself has finite values,
// and a value that reaches itself is the step-1 cycle TypeError.
type Tok = { k: string; kids: Tok[] };
const t: Tok = { k: "a", kids: [{ k: "b", kids: [] }, { k: "c", kids: [{ k: "d", kids: [] }] }] };
console.log(JSON.stringify(t));
console.log(JSON.stringify(t, null, 2));

type Node = { v: number; next: Node | null };
const n: Node = { v: 1, next: { v: 2, next: null } };
console.log(JSON.stringify(n));

type Outer = { name: string; root: Node };
const o: Outer = { name: "o", root: n };
console.log(JSON.stringify(o));

console.log(JSON.stringify([n, n]));

const loop: Node = { v: 9, next: null };
loop.next = loop;
try {
  JSON.stringify(loop);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
