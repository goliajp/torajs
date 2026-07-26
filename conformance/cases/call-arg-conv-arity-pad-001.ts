// The shared argument contract walks the parameter list, but the
// direct-call terminal pads missing trailing `any` params with
// synthesized `undefined` boxes before the walk runs — so argv can be
// longer than the argument list. Those padded positions have no
// argument expression behind them; reaching for one is out of range.

function two(a: any, b: any): void {
  console.log(a, b);
}
two(1);
two(1, 2);

function three(a: number, b: any, c: any): void {
  console.log(a, b, c);
}
three(1.5);
three(1.5, "x");
three(1.5, "x", true);

// A pad behind a converted argument: the first slot still crosses the
// number lanes while the tail is synthesized.
function widened(a: number, b: any): void {
  console.log(a, b);
}
widened(2.5, 0);
widened(7);

// Defaults interleave with pads.
function withDefault(a: number, b: number = 9, c: any = "z"): void {
  console.log(a, b, c);
}
withDefault(1);
withDefault(1, 2);
withDefault(1, 2, 3);
