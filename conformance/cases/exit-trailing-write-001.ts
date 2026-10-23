// The end of the program flushes stdout: a last write not ended by a
// newline still lands.
const parts: string[] = ["a", "b", "c"];
for (const p of parts) process.stdout.write(p);
