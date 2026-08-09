// delete of non-reference operands — §13.5.1.2 step 2: the operand is
// evaluated for its effects and the result is `true`. All strict-legal.
let called = false;
const foo = (): number => {
  called = true;
  return 7;
};
console.log(delete 42);
console.log(delete "s");
console.log(delete true);
console.log(delete null);
console.log(delete foo());
console.log(called);
console.log(delete !0);
console.log(delete (1 + 2));
// parenthesized property reference is still the real delete
const obj: any = { k: 1 };
console.log(delete (obj.k));
console.log(obj.k);
