console.log("lib start");
let count = 0;
count = count + 1;
export function bump(): number {
  count = count + 1;
  return count;
}
export function read(): number {
  return count;
}
console.log("lib end " + count);
