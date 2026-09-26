// a string whose payload is UTF-16 (a view into a UTF-16 parent, or a
// piece cut from one) must compare against an ASCII literal by code
// unit, in `===` / `!==` and in a switch
const wide = "汉 x ab 字q cd"
const hits: string[] = []
for (const t of wide.split(" ")) {
  if (t === "x") hits.push("x")
  if (t === "ab") hits.push("ab")
  if (t !== "cd") hits.push("!" + t.length)
  switch (t) {
    case "x":
      hits.push("sx")
      break
    case "cd":
      hits.push("scd")
      break
    default:
      hits.push("s-")
  }
}
console.log(hits.join(","))

const tail = wide.slice(2, 3)
const mid = wide.substring(4, 6)
console.log(tail === "x", mid === "ab", mid === "ac", tail !== "x")
const parts = wide.split(" ")
console.log(parts[1] === "x", parts[2] === "ab", parts[4] === "cd", parts[3] === "q")
const q = wide.slice(-4, -3)
console.log(q, q === "q", wide.charAt(2) === "x", wide[5] === "b")
