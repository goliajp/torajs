// different: hand-coded forward scan for a(b+)c stands in for a regex engine
package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

// hand-coded matcher for /a(b+)c/g: b+ is greedy and backtracking can only
// hand a 'b' to c, which never matches, so one forward scan is exact
func replaceABC(s string, with string) string {
	var out strings.Builder
	i := 0
	last := 0
	for i < len(s) {
		if s[i] == 'a' {
			j := i + 1
			for j < len(s) && s[j] == 'b' {
				j++
			}
			if j > i+1 && j < len(s) && s[j] == 'c' {
				out.WriteString(s[last:i])
				out.WriteString(with)
				i = j + 1
				last = i
				continue
			}
		}
		i++
	}
	out.WriteString(s[last:])
	return out.String()
}

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	for k := int64(0); k < n; k++ {
		s := "abbc xxx abc yyy abbbbc"
		r := replaceABC(s, "XY")
		total = total + int64(len(r))
	}
	fmt.Println(total)
}
