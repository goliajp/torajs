// Hole Z — for-await element await driven by the ForOf `is_await`
// flag by TYPE, not by a parser-synthesized `.value` Member (which
// conflated the await unwrap with a real user member: a Struct
// element without a `value` field died on member lookup). §27.2 —
// every non-thenable element awaits to itself; Promise(T) elements
// unwrap to T through promise_get_value.

// 1) ident head over struct elements (identity await)
async function identHead() {
  for await (const s of [{ x: 5 }, { x: 6 }]) {
    console.log('ident', s.x);
  }
}

// 2) bare assignment-pattern obj head — the hole-Z minimal repro
async function bareObjHead() {
  const o = { x: 0 };
  for await ({ x: o.x } of [{ x: 5 }]) {
    console.log('bare', o.x);
  }
}

// 3) decl obj pattern head (the test262 async-func-dstr obj family shape)
async function declObjHead() {
  for await (const { x, y } of [{ x: 1, y: 2 }, { x: 3, y: 4 }]) {
    console.log('decl', x, y);
  }
}

// 4) nested obj-over-array decl pattern head
async function nestedHead() {
  for await (const { a: [p, q] } of [{ a: [7, 8] }]) {
    console.log('nested', p, q);
  }
}

// 5) Promise elements still unwrap (regression face of the removed wrap)
async function promiseElems() {
  const pn: Promise<number>[] = [Promise.resolve(10), Promise.resolve(20)];
  for await (const v of pn) {
    console.log('promise', v);
  }
}

async function main() {
  await identHead();
  await bareObjHead();
  await declObjHead();
  await nestedHead();
  await promiseElems();
}
main();
