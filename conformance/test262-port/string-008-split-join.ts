// Adapted from test262: built-ins/String/prototype/split/*.js +
// built-ins/Array/prototype/join — round-trip via a fixed delimiter.
function check(): number {
  let parts: string[] = "a,b,c".split(",");
  if (parts.length !== 3) { throw "#1: len"; }
  if (parts[0] !== "a") { throw "#2: [0]"; }
  if (parts[1] !== "b") { throw "#3: [1]"; }
  if (parts[2] !== "c") { throw "#4: [2]"; }
  if (parts.join("-") !== "a-b-c") { throw "#5: join"; }
  return 0;
}
console.log(check());
