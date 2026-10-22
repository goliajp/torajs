// The twin a typed handler runs on a value its lane cannot hold borrows
// the settled value and hands back an owned answer, which the derived
// promise keeps and releases with itself. A loop of such reactions
// holds no more memory at the end than one does.

async function main() {
  const n = parseInt(process.argv[2] ?? "1000", 10);
  const mark = (v: number): number => v + 1;
  let total = 0;
  for (let i = 0; i < n; i = i + 1) {
    const x: any = "a string long enough to live in its own cell " + i;
    const p: Promise<number> = Promise.resolve(x);
    await p
      .then((v: number) => v + 1)
      .then((r: any) => {
        total = total + r.length;
      });
    await p.then(mark).then((t: any) => {
      total = total + t.length;
    });
  }
  console.log(n, total);
}

main();
