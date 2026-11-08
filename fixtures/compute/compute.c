/* ARCHITECTURE.md section 5.6 worked fixture. Unsigned 32-bit values; test domain n = 0..32. */
typedef unsigned int uint32_t;

uint32_t normalize(uint32_t x)
{
    return x & 255;
}

uint32_t step(uint32_t acc, uint32_t i)
{
    return normalize(acc + i);
}

uint32_t compute(uint32_t n)
{
    uint32_t acc = 0;
    for (uint32_t i = 0; i < n; i++)
        acc = step(acc, i);
    return acc;
}
