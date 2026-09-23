// The assignment half of the same question: the binding is declared
// with a real number and only later assigned the sentinel, inside a
// function body.
const zs: number[] = [1, 2, 3];

function show(p: number): void {
  console.log(typeof p, p === undefined);
}

function outer(): void {
  let a = 0;
  a = zs[9];
  show(a);
}

outer();
