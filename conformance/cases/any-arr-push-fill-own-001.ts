// Chunk 565 (RFC 20260705 ledger #3) — Array<Any> push/fill share
// borrow-shape values (+1 for the slot, source binding keeps its
// stake) and transfer owned temps; the runtime tag is recorded
// truthfully for boxed immediates.
function mk(i: number): string {
  return "hello-" + i;
}

// push a concrete Ident: source survives the slot's lifetime
let s1 = mk(1);
let arr: any[] = [];
arr.push(s1);
arr.pop();
console.log(s1);

// push an owned temp
arr.push(mk(2));
console.log(arr[0]);
arr.pop();

// push an any Ident, discard the slot, read the source
let v3: any = mk(3);
arr.push(v3);
arr.pop();
console.log(v3);

// boxed immediates keep their real tags through push
let vi: any = 42;
let vf: any = 2.5;
let vb: any = true;
arr.push(vi);
arr.push(vf);
arr.push(vb);
console.log(arr[0]);
console.log(arr[1]);
console.log(arr[2]);
console.log(arr.length);

// fill with a concrete Ident: source survives repeated fills
let s4 = mk(4);
let arr2: any[] = [0, 0, 0];
arr2.fill(s4);
arr2.fill(0);
arr2.fill(s4);
console.log(arr2[1]);
console.log(s4);

// fill with an owned temp, then with an any Ident
arr2.fill(mk(5));
console.log(arr2[2]);
let v6: any = mk(6);
arr2.fill(v6);
arr2.fill(7);
console.log(v6);
console.log(arr2[0]);
