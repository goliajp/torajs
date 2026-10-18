// different: hand-written matcher for \bword\b; no regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

const prefix = "a word b xword wordy "

// hand-written matcher for /\bword\b/, not a regex engine
func isWord(c byte) bool {
	return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_'
}

func find(s string) (int, int, bool) {
	const pat = "word"
	if len(s) < len(pat) {
		return 0, 0, false
	}
	for a := 0; a <= len(s)-len(pat); a++ {
		b := a + len(pat)
		if s[a:b] == pat && (a == 0 || !isWord(s[a-1])) && (b == len(s) || !isWord(s[b])) {
			return a, b, true
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
