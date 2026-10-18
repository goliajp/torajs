// suppressed: each concat is stored to a package-level sink so the per-iteration string stays a heap allocation, as black_box keeps it in main.rs
package main

import (
	"fmt"
	"os"
	"strconv"
)

var sink string

func main() {
	prefix := "  Hello42 word  "
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	for i := int64(0); i < n; i++ {
		// prefix + 'x' builds a fresh string of exactly the combined length
		sink = prefix + "x"
		total = total + int64(len(sink))
	}
	fmt.Println(total)
}
