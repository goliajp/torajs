// Integration: array of class instances, iterated via for-loop.
class Box {
  size: number;
  constructor(s: number) { this.size = s; }
  area(): number { return this.size * this.size; }
}

function check(): number {
  let boxes: Box[] = [new Box(2), new Box(3), new Box(4)];
  let total: number = 0;
  for (let i: number = 0; i < boxes.length; i = i + 1) {
    total = total + boxes[i].area();
  }
  // 4 + 9 + 16 = 29
  if (total !== 29) { throw "#1"; }
  return 0;
}
console.log(check());
