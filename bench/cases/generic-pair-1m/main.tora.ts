type Pair<A, B> = { fst: A, snd: B };

function loopSum(n: number): number {
  let sum: number = 0;
  for (let i: number = 0; i < n; i = i + 1) {
    let p: Pair<number, number> = { fst: i, snd: i + 1 };
    sum = sum + p.fst + p.snd;
  }
  return sum;
}

console.log(loopSum(1000000));
