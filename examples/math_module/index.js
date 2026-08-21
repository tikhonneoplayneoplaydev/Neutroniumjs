// Pack with: neut pack examples/math_module --output examples/math.nim
// Inspect with: neut manifest examples/math.nim
export function add(a, b) {
    return a + b;
}

export function square(x) {
    return x * x;
}
