// two same-scope copies of one binding — three independent stakes,
// all readable, each dropped once at scope close.
function main(): void {
  const s: string = "triple";
  const t: string = s;
  const u: string = s;
  console.log(s);
  console.log(t);
  console.log(u);
}

main();
