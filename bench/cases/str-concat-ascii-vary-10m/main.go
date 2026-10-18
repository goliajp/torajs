package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	prefix := "  Hello42 word  "
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	tails := [2]string{os.Args[2], os.Args[3]}
	ring := make([]string, 8)
	var total int64 = 0
	for i := int64(0); i < n; i++ {
		// prefix + tail builds a fresh string of exactly the combined length
		s := prefix + tails[i&1]
		total += int64(s[i&15])
		ring[i&7] = s
	}
	fmt.Println(total)
	fmt.Println(ring[(n-1)&7])
}
