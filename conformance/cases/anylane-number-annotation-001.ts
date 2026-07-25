// A `number` annotation on a binding whose init is `any` decodes the
// box (§7.1.4 ToNumber) instead of storing its bits. Every `any`
// source shape reaches the same binding boundary.

const bare: any = 10;
const a: number = bare;
console.log("bare ident   :", a);

const obj: any = { v: 41, w: 1.5 };
const b: number = obj.v;
console.log("member       :", b);

const c: number = obj["v"];
console.log("index-by-name:", c);

const arr: any = [7, 8];
const d: number = arr[1];
console.log("index        :", d);

function mkAny(): any {
  return 12.5;
}
const e: number = mkAny();
console.log("call return  :", e);

const f: number = obj.w;
console.log("fractional   :", f);

// arithmetic reads the decoded number, not the box
const g: number = obj.v + 1;
console.log("arith        :", g);

const h: number = obj.v;
const i: number = obj.v;
console.log("two bindings :", h + i);

// `let` takes the same row as `const`
let j: number = obj.v;
j = j + 1;
console.log("let          :", j);

// a method on an `any` receiver: `this` is `any`, so `this.v` is the
// same crossing one frame in
const withMethod: any = {
  v: 10,
  read() {
    const local: number = this.v;
    return local;
  },
};
console.log("this in方法   :", withMethod.read());

// and inside `[Symbol.iterator]`, where a user-written iterator most
// naturally reaches for `this`
const counter: any = {
  limit: 3,
  [Symbol.iterator]() {
    const stop: number = this.limit;
    let n = 0;
    return {
      next() {
        n = n + 1;
        return n <= stop ? { value: n, done: false } : { value: 0, done: true };
      },
    };
  },
};
const seen: number[] = [];
for (const v of counter) {
  seen.push(v);
}
console.log("@@iterator   :", seen.join(","));
