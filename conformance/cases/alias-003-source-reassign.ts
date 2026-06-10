// reassigning the source must not disturb the copy: `t` holds its own
// stake on the old heap, `s = "new"` drops only s's stake.
function main(): void {
  let s: string = "old-value";
  const t: string = s;
  s = "new-value";
  console.log(t);
  console.log(s);
}

main();
