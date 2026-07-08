// Chunk 703 — `obj?.[index]` optional element access (ES2020
// §13.3.9). Previously a parse error ("expected identifier after
// `?.`, got LBracket"). The receiver evaluates once; a nullish
// receiver short-circuits to undefined WITHOUT evaluating the index
// expression; `a?.["k"]` with an identifier-name literal folds to
// the member-access shape at parse time.
const m = /a(b)?/.exec("a");
console.log(m?.[0]);
console.log(m?.[1]);
const miss = /x/.exec("a");
console.log(miss?.[0]);
// any chains (mixed inner literal — the dynobj-field typed-array
// kind-mark gap is a separate pre-existing L3b face)
const o: any = { items: [10, "s", 20] };
console.log(o?.items?.[2]);
console.log(o.missing?.[0]);
// spec: the index expression must NOT evaluate on the short-circuit
function probeShortCircuit(): void {
  let calls = 0;
  const step = (): number => {
    calls++;
    return 0;
  };
  const n: any = null;
  console.log(n?.[step()]);
  console.log(calls);
  const u: any = undefined;
  console.log(u?.[step()]);
  console.log(calls);
}
probeShortCircuit();
// plain non-null receivers: ?.[] ≡ []
const arr: number[] = [7, 8];
console.log(arr?.[0]);
const s = "hello";
console.log(s?.[1]);
// identifier-name string literal folds to member access
console.log(o?.["items"]?.[0]);
// dynamic string key probes by runtime cell
const k = "items";
console.log(o?.[k]?.[0]);
// typeof rides the Any box
console.log(typeof arr?.[0], typeof (o.missing?.[3]));
