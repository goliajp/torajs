// shift on element types beyond number and string: booleans, class
// instances (the slot's stake moves to the result), nested arrays,
// and a shift whose result is dropped unread
const flags: boolean[] = []
for (let i = 0; i < 30; i = i + 1) {
  flags.push(i % 3 === 0)
  if (flags.length > 4) {
    const f = flags.shift()!
    if (i % 7 === 0) console.log(i, f)
  }
}
console.log(flags.length, flags.join(","))

class Job {
  constructor(public id: number, public name: string) {}
}
const jobs: Job[] = []
let seen = 0
for (let i = 0; i < 50; i = i + 1) {
  jobs.push(new Job(i, "job" + i))
  if (jobs.length > 6) {
    const j = jobs.shift()!
    seen = seen + j.id
    if (i % 10 === 0) console.log(j.id, j.name)
  }
}
console.log(seen, jobs.length, jobs[0].name, jobs[5].name)

const rows: number[][] = [[1, 2], [3], [4, 5, 6]]
const r = rows.shift()!
r.push(9)
console.log(r.join(","), rows.length, rows[0].join(","), rows[1].length)

const xs: number[] = [10, 20, 30, 40]
xs.shift()
xs.shift()
console.log(xs.length, xs[0], xs[1], xs.indexOf(40))
