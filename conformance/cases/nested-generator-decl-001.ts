// 393-03 — nested `function*` declarations (block / fn body) lift
// to the top level before the generator desugar claims its decls;
// the parser-baked for-of fast-path annotations (`__Gen_<name>`)
// remap alongside. Pre-fix every nested generator died loud at
// check ("yield is only valid inside a `function*` generator body").
{
  function* g() { yield 1 }
  console.log([...g()]);
}
function f(): number[] {
  function* g2() { yield 2 }
  return [...g2()];
}
console.log(f());
function outer(): number[] {
  function* inner(n: number) { yield n; yield* tail() }
  function* tail() { yield 99 }
  const acc: number[] = [];
  for (const v of inner(5)) acc.push(v);
  for (const v of inner(6)) acc.push(v);
  return acc;
}
console.log(outer());
{
  {
    function* deep() { yield 3; yield 4 }
    console.log([...deep()]);
  }
}
if (true) {
  function* cond() { yield 7 }
  console.log([...cond()]);
}
class M { *gen() { yield 8 } }
console.log([...new M().gen()]);
