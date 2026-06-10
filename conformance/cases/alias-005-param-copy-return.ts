// copying a param binding then returning the copy: the caller's
// original stays alive after the call result is consumed.
function viaParam(p: string): string {
  const t: string = p;
  return t;
}

function main(): void {
  const msg: string = "param-copy";
  const r: string = viaParam(msg);
  console.log(r);
  console.log(msg);
}

main();
