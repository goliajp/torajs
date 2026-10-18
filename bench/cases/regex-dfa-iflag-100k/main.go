// different: hand-written ascii case-folding scan for hello; no regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "before HELLO world "

func lowerASCII(c byte) byte {
	if c >= 'A' && c <= 'Z' {
		return c + ('a' - 'A')
	}
	return c
}

// hand-written matcher for /hello/i, not a regex engine; without the u flag js folds
// no non-ascii character onto an ascii one, so ascii case folding is exact here
func find(s string) (int, int, bool) {
	const pat = "hello"
	if len(s) < len(pat) {
		return 0, 0, false
	}
	for a := 0; a <= len(s)-len(pat); a++ {
		k := 0
		for k < len(pat) && lowerASCII(s[a+k]) == pat[k] {
			k++
		}
		if k == len(pat) {
			return a, a + len(pat), true
		}
	}
	return 0, 0, false
}

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	var buf []byte
	for i := int64(0); i < n; i++ {
		buf = append(buf[:0], prefix...)
		buf = strconv.AppendInt(buf, i, 10)
		s := string(buf)
		if a, b, ok := find(s); ok {
			m0 := strings.Clone(s[a:b])
			total += int64(len(m0))
		}
	}
	fmt.Println(total)
}
