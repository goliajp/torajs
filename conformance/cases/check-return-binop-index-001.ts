// root-position elem-borrow return must still pin its receiver
function nested(): any {
  const matrix: any[] = [[1, 2, 3], [4, 5]];
  return matrix[0];
}
const row = nested();
console.log(row[0], row[1], row[2], row.length);

// chunk-674 root form: any-elem return retains
function first(): any {
  const a: any[] = ["str-elem-payload", 7];
  return a[0];
}
console.log(first());

// binop-index form (chunk 718): fresh result, receiver drops
function pick(): number {
  const a: any[] = ["x1x2x3x4x5", 20, 30];
  return a[1] + a[2];
}
console.log(pick());

// binop with string elems: concat is fresh
function cat(): any {
  const a: any[] = ["left-", "right"];
  return a[0] + a[1];
}
console.log(cat());

// mixed operand: ident side keeps the conservative walk
function mix(): any {
  const a: any[] = ["idx-", 1];
  const s = "ident-side";
  return a[0] + s;
}
console.log(mix());

// nested binop chain
function chain(): number {
  const a: any[] = [1, 2, 3];
  return a[0] + a[1] + a[2];
}
console.log(chain());
