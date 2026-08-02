// Generator-lifted desugar temps ride the any lane
// (desugar_generators_walkers::lift_lets_in_stmt): the parser's
// for-of source hoist (__forof_src_N), destructured loop element
// (__forof_destr_N) and pattern-unpack aliases (__ary_src_N /
// __nested_destr_N) carry no annotation — the historical "number"
// lift fallback pinned the fields against their array inits (t262
// for-await-of dstr family, 366-case cluster). The sync generator
// leg runs first so the async leg's microtask ordering matches.
function* g() {
  for (const [a] of [[7], [8]]) {
    yield a;
  }
}
for (const v of g()) console.log(v);
let v2: any, vNull: any;
let iterCount = 0;
async function* fn() {
  for await ([v2 = 10, vNull = 11] of [[2, null]]) {
    console.log(v2, vNull);
    iterCount += 1;
  }
}
async function go() {
  const it: any = fn();
  await it.next();
  await it.next();
  console.log(iterCount);
}
go();
