vec3 colormap(vec3 t) {
    vec3 nt = normalize(t);

    return vec3((nt.r + 1.0) * 0.5, (nt.b + 1.0) * 0.5, -nt.g + 0.5);
}