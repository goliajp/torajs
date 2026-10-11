console.log("lib body");
let calls = 0;
export default function hello(): string {
  calls = calls + 1;
  return "hello " + calls;
}
export const tag = "t";
console.log("lib done");
