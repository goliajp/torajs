// the only writer of an exotic index here is `delete` on a typed
// cell; the typed sort must take the per-index walk (§23.1.3.30)
const cut: number[] = [5, 3, 9, 1, 7]
delete cut[1]
cut.sort((p, q) => p - q)
console.log(cut.length, 1 in cut, 4 in cut, cut[0], cut[1], cut[2], cut[3], cut[4])
