// `await v` of a value that is no promise answers `v` itself
// (ECMA-262 §27.7.5.3 over PromiseResolve of a non-thenable), so a
// fraction keeps its fraction in the binding that receives it, an
// annotated one included, and in an array it is pushed into.

async function literal() {
  const xs: number[] = [];
  const v: number = await 1.5;
  xs.push(v);
  const w: number = await 2;
  xs.push(w);
  console.log("literal", xs[0], xs[1]);
}

async function binding(x: number) {
  const ys: number[] = [];
  const v = await x;
  ys.push(v);
  console.log("binding", v, ys[0]);
}

async function main() {
  await literal();
  await binding(2.25);
  await binding(3);
}
main();
