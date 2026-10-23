// Promise.all over arrays of genuinely typed promises: the result keeps
// its typed element lane (integers, fractions, strings), next to an
// array of the same annotation that came from `any` in the same program.
function fromAny(): any {
  return [Promise.resolve(1), { then(res: any) { res("x"); } }];
}
async function main() {
  const ints: Promise<number>[] = [Promise.resolve(4), Promise.resolve(5)];
  const ri = await Promise.all(ints);
  console.log(ri[0] + ri[1], ri.length);
  const fracs: Promise<number>[] = [Promise.resolve(0.5), Promise.resolve(1.25)];
  const rf = await Promise.all(fracs);
  console.log(rf[0] + rf[1]);
  const strs: Promise<string>[] = [Promise.resolve("a"), Promise.resolve("b")];
  const rs = await Promise.all(strs);
  console.log(rs.join("-"));
  const loose: Promise<number>[] = fromAny();
  const rl = await Promise.all(loose);
  console.log(rl[0], rl[1]);
}
main();
