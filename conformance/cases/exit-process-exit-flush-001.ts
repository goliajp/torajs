// process.exit leaves through the same exit as the end of the program,
// which flushes stdout first: output not ended by a newline still lands.
console.log("line before");
process.stdout.write("no newline before exit");
process.exit(0);
console.log("not reached");
