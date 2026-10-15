// A value the slot has no spelling for, written through the any lane,
// sits in the field's sidecar (a fraction in an integer slot, a string
// in a number slot); the next typed store replaces it, in a program
// that names no definer and never freezes.
class Box {
  n: number
  constructor(n: number) {
    this.n = n
  }
}

const d = new Box(6)
const dd: any = d
dd.n = 2.5
console.log(d.n)
d.n = 7
console.log(d.n, dd.n)
dd.n = "s"
console.log(dd.n)
d.n = 9
console.log(d.n + 1, dd.n)
