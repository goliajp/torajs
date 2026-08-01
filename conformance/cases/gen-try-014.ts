// async generator: return() routes through finally too (D3b delegate
// wrapped by the async machinery)
let log1: string[] = [];
async function* ag() {
  try {
    yield 1;
    yield 2;
  } finally {
    log1.push("fin");
  }
}
let log2: string[] = [];
async function* ag2() {
  try {
    yield "a";
  } finally {
    log2.push("f2");
    yield "cleanup";
  }
}
async function main() {
  const it = ag();
  const a = await it.next();
  console.log(a.value, a.done);
  const b = await it.return(9);
  console.log(b.value, b.done);
  console.log(log1.join(","));
  const c = await it.next();
  console.log(c.value, c.done);

  const jt = ag2();
  const d = await jt.next();
  console.log(d.value, d.done);
  const e = await jt.return(7);
  console.log(e.value, e.done);
  const f = await jt.next();
  console.log(f.value, f.done);
  console.log(log2.join(","));
}
main();
