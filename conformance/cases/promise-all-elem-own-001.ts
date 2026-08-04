// Promise.all's result array co-owns its elements.
//
// The result array is heap-chained, so its last owner drops every
// slot. Each source promise keeps its own stake on the value it
// settled with, so the result has to pay for the one it keeps. It
// used not to: the sources died at the end of each iteration, freed
// the strings, and the array kept pointing at the freed blocks — so
// every read below answered whatever the allocator had since put
// there (in practice the LAST iteration's strings, for all ten).
// Silently wrong values, not a crash.

async function main() {
  const mk = (v: string): Promise<string> => Promise.resolve(v + "-tail");
  const kept: string[] = [];
  for (let i = 0; i < 5; i++) {
    const pair = await Promise.all([mk("x" + i), mk("y" + i)]);
    kept.push(pair[0]);
    kept.push(pair[1]);
  }
  for (let i = 0; i < kept.length; i++) {
    console.log(i, kept[i]);
  }
}

main();
