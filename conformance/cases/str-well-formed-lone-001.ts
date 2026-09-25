// isWellFormed / toWellFormed see lone surrogates a string can hold:
// made by concatenation, by slicing a pair apart, or written as an
// escape, on both the typed and the any receiver

const lead = "\uD83D"
const trail = "\uDCA9"
const pair = lead + trail
const typed: string[] = [
  "a" + lead + "c",
  "a" + trail + "c",
  "a" + trail + lead + "d",
  "a" + pair + "d",
  pair.slice(0, 1),
  pair.slice(1),
  "abc",
  "a▨c",
  "💩",
]
for (const s of typed) {
  const w = s.toWellFormed()
  const codes: number[] = []
  for (let i = 0; i < w.length; i++) codes.push(w.charCodeAt(i))
  console.log(s.isWellFormed(), w.length, codes.join(","))
}

var anyLead: any = lead
var dyn: any = "x" + anyLead + "y"
console.log(dyn.isWellFormed(), dyn.toWellFormed().charCodeAt(1))
console.log(("a" + lead + "c" + lead + "e").isWellFormed(), (pair + "!").isWellFormed())
