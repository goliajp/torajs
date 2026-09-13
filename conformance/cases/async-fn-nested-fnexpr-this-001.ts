// The move gate stops at a function expression: its `this` is its own,
// not the async body's, so a body holding one still moves and suspends
// at its first await (`b` prints between `a` and `c`).
async function f(): Promise<any> {
  const inner = function (this: any) { return this.w; };
  console.log("a");
  await null;
  console.log("c");
  return inner.call({ w: 4 });
}
f().then((x: any) => console.log("f", x));
console.log("b");
