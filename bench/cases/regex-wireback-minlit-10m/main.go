// different: hand-written byte search for x; no match array, index or input props
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

// hand-written matcher for /x/, not a regex engine
func find(s string) (int, int, bool) {
	a := strings.IndexByte(s, 'x')
	if a < 0 {
		return 0, 0, false
	}
	return a, a + 1, true
}

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	for i := int64(0); i < n; i++ {
		s := "x"
		if a, b, ok := find(s); ok {
			// the match array's group 0 is a fresh string in js too
			_ = strings.Clone(s[a:b])
			total += 1
		}
	}
	fmt.Println(total)
}
