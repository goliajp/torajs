// A list class whose constructor can throw before it writes the next
// link: the half-built node is released with the link still holding
// what the field held before its constructor ran, `undefined`. The
// release keeps testing every cell it takes for the undefined sentinel.
class Link {
  v: number
  next: Link | undefined
  constructor(v: number, next: Link | undefined, fail: boolean) {
    this.v = v
    if (fail) throw 'bad link ' + v
    this.next = next
  }
}
function len(l: Link | undefined): number {
  let n = 0
  while (l !== undefined) {
    l = l.next
    n++
  }
  return n
}
let made = 0
let failed = 0
let head: Link | undefined = new Link(0, undefined, false)
for (let i = 1; i < 3000; i++) {
  try {
    head = new Link(i, head, i % 7 === 0)
    made++
  } catch (e) {
    failed++
  }
  if (i % 500 === 0) head = new Link(i, undefined, false)
}
console.log(made, failed, len(head))
