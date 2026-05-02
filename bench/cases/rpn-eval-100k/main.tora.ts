function evalRpn(expr: string): number {
  let stack: number[] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  let sp: number = 0;
  let parts: string[] = expr.split(" ");
  for (let i: number = 0; i < parts.length; i = i + 1) {
    let tok: string = parts[i];
    let c0: number = tok.charCodeAt(0);
    if (c0 >= 48 && c0 <= 57) {
      stack[sp] = c0 - 48;
      sp = sp + 1;
    } else {
      let b: number = stack[sp - 1];
      let a: number = stack[sp - 2];
      sp = sp - 2;
      let r: number = 0;
      if (c0 === 43) { r = a + b; }
      else if (c0 === 45) { r = a - b; }
      else { r = a * b; }
      stack[sp] = r;
      sp = sp + 1;
    }
  }
  return stack[0];
}

let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  total = total + evalRpn("3 4 + 2 * 5 +");
}
console.log(total);
