// rotation 552 (551-06) — `for (const v of gen())` over a generator CALL
// used to be desugared at parse time into a bare `next()` loop with no
// IteratorClose, so `break` / `return` / a throw out of the loop never
// ran the generator's `return()` and its `finally` never ran — while the
// same loop over `const it = gen()` closed correctly. Both spellings now
// take the iterator-protocol lane, which runs §7.4.9 on every exit.
const log: string[] = [];
function* gen(tag: string): Generator<number> {
  try {
    yield 1;
    yield 2;
    yield 3;
  } finally {
    log.push("closed:" + tag);
  }
}
function* plain(): Generator<string> {
  yield "a";
  yield "b";
}

// 1. break out of a call-source loop
for (const v of gen("brk")) {
  if (v === 2) break;
}
console.log(log.join(" "));

// 2. return out of two loops, ident and call source
const f = (): number => {
  const it = gen("ret-ident");
  for (const v of it) {
    for (const w of gen("ret-call")) {
      if (v === 1 && w === 1) return v + w;
    }
  }
  return -1;
};
console.log(f(), log.join(" "));

// 3. a throw from the body closes, then reaches the catch
try {
  for (const v of gen("thr")) {
    if (v === 1) throw new Error("t" + v);
  }
} catch (e) {
  console.log((e as Error).message, log.join(" "));
}

// 4. labeled continue / break out of an inner call-source loop
let seen = 0;
outer: for (let i = 0; i < 2; i++) {
  for (const v of gen("lbl" + i)) {
    seen += v;
    if (v === 1) continue outer;
  }
}
console.log(seen, log.join(" "));

// 5. natural completion closes exactly once, a string generator still
//    yields, and a manual return() still works
let sum = 0;
for (const v of gen("full")) sum += v;
const parts: string[] = [];
for (const s of plain()) parts.push(s);
const it2 = gen("manual");
it2.next();
console.log(sum, parts.join(","), JSON.stringify(it2.return(9)), log.join(" "));
