/*
 * Argo front end fixture.
 *
 * Small on purpose: one enum, one struct, two functions, and every control
 * construct the block templates in ARCHITECTURE.md section 5 name - an
 * if/else chain, a switch, a for loop and a while loop. No preprocessor
 * directives, because this build feeds the file straight to the parser
 * without running rucc-pp first.
 */

enum bus_width {
    BUS_8 = 1,
    BUS_16 = 2,
    BUS_32 = 4,
    BUS_64 = 8,
};

struct ram {
    unsigned char *data;
    unsigned long size;
    unsigned long base;
};

static inline unsigned long mask_for(enum bus_width w)
{
    return (unsigned long)w - 1;
}

int ram_read(struct ram *r, unsigned long addr, enum bus_width w, unsigned long *out)
{
    unsigned long offset;

    if (r == 0 || r->data == 0)
        return -1;

    offset = addr - r->base;
    if (offset >= r->size)
        return -1;

    *out = 0;
    switch (w) {
    case BUS_8:
        *out = r->data[offset];
        break;
    case BUS_16:
        *out = r->data[offset] | ((unsigned long)r->data[offset + 1] << 8);
        break;
    default:
        return -2;
    }

    return 0;
}

void ram_fill(struct ram *r, unsigned char v)
{
    unsigned long i;

    for (i = 0; i < r->size; i++)
        r->data[i] = v;

    while (i > 0)
        i--;

    (void)mask_for(BUS_32);
}
