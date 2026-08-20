// rotation 460 — the array-literal ANY slot takes its +1 through the
// FUSED owned unbox (`any_unbox_value_owned` = unbox + payload
// rc_inc), so an OWNED any temp in an element position is left
// holding a stake nobody releases while a BORROWED binding must keep
// its own. The chunk-610 carve-out read the fused inc as a transfer
// and excluded the whole type: `function mk(n): any { return "s" + n }
// … [mk(i)]` leaked one cell per iteration (13.2MB vs 6.8MB RSS over
// 200k) while the `: string`-returning twin stayed flat.
function anyResult(n: number): any {
  return "s" + n;
}
let fromCall: any[] = [anyResult(6)];
let held: any = "s7";
let fromBinding: any[] = [held];
console.log(fromCall[0], fromBinding[0], held);

// Two elements, mixed provenance — the owned one releases, the
// borrowed one does not, and both slots read back live.
let mixed: any[] = [anyResult(8), held, anyResult(9)];
console.log(mixed.join(","), held);
