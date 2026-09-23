// A binding declared inside a function body can hold the out-of-range
// sentinel just as a top-level one can (§10.1.8.1 OrdinaryGet step
// 2.b answers undefined either way), and handing it to a typed
// parameter must not turn it into a plain number.
const zs: number[] = [1, 2, 3];

function show(p: number): void {
  console.log(typeof p, p === undefined);
}

function outer(): void {
  const u = zs[9];
  show(u);
}

outer();
