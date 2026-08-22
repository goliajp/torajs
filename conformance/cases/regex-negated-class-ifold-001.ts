// ES canonicalizes the input and then asks for membership, so a
// negated class under `i` rejects both cases of what it lists:
// `/[^ab]/i` says no to "A". Building the complement over the
// unfolded set let "A" through — under `u` that had always been the
// answer, and it stayed wrong there while the flagless form was
// still stepping bytes.
const probes: boolean[] = [
  /[^ab]/i.test("A"), /[^ab]/i.test("c"), /(?i:[^ab])/.test("A"),
  /[^ab]/iu.test("A"), /[^ab]/iu.test("c"), /(?i:[^ab])/u.test("A"),
  /[ab]/i.test("A"), /[ab]/iu.test("A"),
  /[^a-c]/i.test("B"), /[^a-c]/i.test("D"),
  /[^é]/i.test("é"), /[^é]/iu.test("é"), /[^é]/i.test("x"),
  /[^ab]/.test("A"), /[^ab]/u.test("A"),
];
for (const p of probes) console.log(p);
