// a yield in a comma-separated loop test or step runs after the operands before it, in a generator and an async function
function* step() {
  for (let i = 0; i < 3; i++, yield i) {}
}
console.log([...step()].join(" "));
function* stepThenMore() {
  let i = 0, j = 0;
  for (; j++, i < 3; i++, yield i * 10, j += 100) {}
  yield j;
}
console.log([...stepThenMore()].join(" "));
function* stepTwice() {
  for (let i = 0; i < 3; i++, yield i, i++) {}
}
console.log([...stepTwice()].join(" "));
function* whileTest() {
  let i = 0;
  while (i++, (yield i) !== undefined || i < 3) {}
}
console.log([...whileTest()].join(" "));
function* doWhileTest() {
  let i = 0;
  do {} while (i++, (yield i) === undefined && i < 3);
}
console.log([...doWhileTest()].join(" "));
function* forTest() {
  for (let i = 0; (yield i), i < 2; i++) {}
}
console.log([...forTest()].join(" "));
async function awaitStep(): Promise<string> {
  const out: number[] = [];
  let i = 0;
  for (let k = 0; k < 3; i++, out.push(await Promise.resolve(i))) {
    k++;
  }
  return out.join(",");
}
awaitStep().then(console.log);
