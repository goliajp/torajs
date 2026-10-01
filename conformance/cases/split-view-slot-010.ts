// An `any`-annotated member of the alias class: the receiver decodes
// owned strings, so the class holds owned elements and the product is
// materialized where it is declared.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('vvvvvvvvv' + i)
}
const lines: string[] = []
for (let i = 0; i < 3; i++) lines.push('a' + i + ',b' + i)
let anyLast: any = []
for (let i = 0; i < lines.length; i++) {
  const parts = lines[i].split(',')
  anyLast = parts
}
churn()
console.log(anyLast.length, anyLast[0], JSON.stringify(anyLast))
const out: any[] = []
for (const e of anyLast) out.push(e + '!')
console.log(JSON.stringify(out), anyLast[1])
