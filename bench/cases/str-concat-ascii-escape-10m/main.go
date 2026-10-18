// different: the concat is loop-invariant; tr and bun build it once before the loop, this program builds a fresh string every iteration (str-concat-ascii-vary-10m is the case where every runtime must build it)
package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	prefix := "  Hello42 word  "
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	tail := os.Args[2]
	ring := make([]string, 8)
	var total int64 = 0
	for i := int64(0); i < n; i++ {
		// prefix + tail builds a fresh string of exactly the combined length
		s := prefix + tail
		total += int64(s[i&15])
		ring[i&7] = s
	}
	fmt.Println(total)
	fmt.Println(ring[(n-1)&7])
}
