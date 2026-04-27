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


total = 0
for i in range(200):
    for j in range(200):
        cr = i / 100 - 1.5
        ci = j / 100 - 1.0
        total += mandel(cr, ci, 1000)

print(total)
