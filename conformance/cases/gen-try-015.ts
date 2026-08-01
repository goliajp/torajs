// §27.6.3.7 step 8.d: async generator return(v) awaits v before
// completing with it (suspendedYield, suspendedStart, and through
// finally); non-thenable values pass through identity
let log: string[] = [];
async function* ag() { yield 1; }
async function* agf() {
  try {
    yield 1;
  } finally {
    log.push("fin");
  }
}
async function main() {
  const it = ag();
  await it.next();
  const s = await it.return(Promise.resolve(9));
  console.log(s.value, s.done);

  const it2 = ag();
  const t = await it2.return(Promise.resolve(5));
  console.log(t.value, t.done);

  const it3 = ag();
  await it3.next();
  const u = await it3.return(7);
  console.log(u.value, u.done);

  const it4 = agf();
  await it4.next();
  const v = await it4.return(Promise.resolve(3));
  console.log(v.value, v.done);
  console.log(log.join(","));
}
main();
