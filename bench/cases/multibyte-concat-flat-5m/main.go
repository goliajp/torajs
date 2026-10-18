package main

import (
	"fmt"
	"os"
	"strconv"
)

// js strings are UTF-16 and .length counts code units, so the string is a
// []uint16 here; a UTF-8 string would copy 3 bytes per char instead of 2 and
// need a scan to get the code-unit length
//
// "中文测试" as UTF-16 code units, a constant as the literal is in main.ts
var piece = [4]uint16{0x4e2d, 0x6587, 0x6d4b, 0x8bd5}

func main() {
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	pieces, _ := strconv.ParseInt(os.Args[2], 10, 32)
	for k := int64(0); k < n; k++ {
		// capacity doubling from 16 code units, as main.c grows its buffer
		acc := make([]uint16, 0, 16)
		for p := int64(0); p < pieces; p++ {
			// acc = acc + piece on a uniquely owned acc appends in place
			acc = append(acc, piece[:]...)
		}
		total = total + int64(len(acc)) + int64(acc[len(acc)-1])
	}
	fmt.Println(total)
}
