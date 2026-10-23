// Promise.allSettled over an array that reached a `Promise<number>[]`
// slot from an `any` value: each record's `value` is what the element
// settled with, a string from a thenable included, not the record field
// read at the width the annotation names.
function mixed(): any {
  return [Promise.resolve(1), { then(res: any) { res("two"); } }, 3.5];
}
async function main() {
  const a: Promise<number>[] = mixed();
  const s = await Promise.allSettled(a);
  console.log(s.length, s[0].status, s[1].status, JSON.stringify(s));
  console.log("race", await Promise.race(a));
  console.log("any", await Promise.any(a));
}
main();
