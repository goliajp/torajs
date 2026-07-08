// chunk 713 — wrong-arm optional call short-circuits without arg eval
let calls = 0;
function f(): number { calls += 1; return 1; }
const n: any = 42;
console.log(n.slice?.(f()));
console.log("calls:", calls);
const s: any = "hi";
console.log(s.toUpperCase?.());
console.log(s.push?.(f()));
console.log("calls:", calls);
const arr: any = [1];
console.log(arr.push?.(5));
console.log(arr.length);
const b: any = true;
console.log(b.toString?.());
class Q { z: number = 3; }
const q: any = new Q();
console.log(q.nomethod?.(f()));
console.log("calls:", calls);
