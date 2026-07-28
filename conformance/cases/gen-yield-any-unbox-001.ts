function* g(): number {
  const x: any = 42;
  yield x;
  const y: any = 7;
  yield y + 1;
}
for (const v of g()) console.log(v);

function* h(): number {
  const r: any = 99;
  return r;
}
const it = h();
const step = it.next();
console.log(step.value);
console.log(step.done);
