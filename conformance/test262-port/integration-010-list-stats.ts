// Integration: a list-of-stats pipeline that exercises classes,
// arrays, methods on each instance, and aggregate over the list.
class Sample {
  value: number;
  weight: number;
  constructor(v: number, w: number) {
    this.value = v;
    this.weight = w;
  }
  weighted(): number { return this.value * this.weight; }
}

function totalWeighted(samples: Sample[]): number {
  let s: number = 0;
  for (let i: number = 0; i < samples.length; i = i + 1) {
    s = s + samples[i].weighted();
  }
  return s;
}

function totalWeight(samples: Sample[]): number {
  let s: number = 0;
  for (let i: number = 0; i < samples.length; i = i + 1) {
    s = s + samples[i].weight;
  }
  return s;
}

function check(): number {
  let xs: Sample[] = [
    new Sample(10, 1),
    new Sample(20, 2),
    new Sample(30, 3),
  ];
  // Σ(value × weight) = 10 + 40 + 90 = 140
  if (totalWeighted(xs) !== 140) { throw "#1"; }
  // Σ(weight) = 6
  if (totalWeight(xs) !== 6) { throw "#2"; }
  // both fns called on the same list = TS-shape borrow worked
  if (totalWeighted(xs) + totalWeight(xs) !== 146) { throw "#3"; }
  return 0;
}
console.log(check());
