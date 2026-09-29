import sys

def mandel(cr, ci, max_iter):
    zr = 0.0
    zi = 0.0
    n = 0
    while n < max_iter:
        if zr * zr + zi * zi > 4:
            return n
        new_zr = zr * zr - zi * zi + cr
        zi = 2 * zr * zi + ci
        zr = new_zr
        n += 1
    return max_iter


side = int(sys.argv[1])
max_iter = int(sys.argv[2])
total = 0
for i in range(side):
    for j in range(side):
        cr = i / 100 - 1.5
        ci = j / 100 - 1.0
        total += mandel(cr, ci, max_iter)

print(total)
