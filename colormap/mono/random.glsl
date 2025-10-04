float hash(float n) {
    return fract(sin(n) * 43758.5453123);
}

vec3 colormap(float t) {
    // Spread t into three separate hash ranges to decorrelate R, G, B
    float r = hash(t);
    float g = hash(t + 17.123);
    float b = hash(t + 51.913);

    return vec3(r, g, b);
}