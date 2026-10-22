// A handler that meets a value its lane cannot hold, held in a binding
// or given a number face that makes the `.then` lowering wrap it in a
// bits adapter: the reaction finds its any twin through the module's
// handler table and runs it, so the body sees the value as it is
// (§27.2.5.4, §10.2.1.3). A value that fits still takes the typed body.

const str: any = "str";
const num: any = 8;

async function main() {
  const p: Promise<number> = Promise.resolve(str);
  const q: Promise<number> = Promise.resolve(num);

  const held = (v: number) => {
    console.log("held", typeof v, v);
  };
  await p.then(held);
  await q.then(held);

  const scale = 3;
  const times = (v: number): number => v * scale;
  console.log("times", await p.then(times));
  console.log("times fits", await q.then(times));

  const suffix = (v: number): number => v + 1;
  await p.then(suffix).then((r: any) => {
    console.log("suffix", typeof r, r);
  });
  console.log("suffix fits", await q.then(suffix));

  const neg: Promise<number> = Promise.reject(str);
  await neg.catch(held);
  await neg.then(suffix, held);
}

main();
