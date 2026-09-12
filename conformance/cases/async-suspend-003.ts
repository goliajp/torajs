// RFC 20260805 blade 1 — an `await` resumes after ONE tick, so it lands
// between the first and second link of an independent `.then` chain
// rather than after the whole queue has drained.
Promise.resolve(0)
  .then(() => { console.log("c1"); })
  .then(() => { console.log("c2"); })
  .then(() => { console.log("c3"); });

async function main(): Promise<void> {
  await Promise.resolve(1);
  console.log("AWAKE");
}

main();
