vec3 colormap(float t) {
    if (isnan(t)) {
        return vec3(1.0, 0.0, 1.0); // Magenta for NaN
    } else if (isinf(t)) {
        if (t > 0.0) {
            return vec3(1.0, 1.0, 0.0); // Yellow for +Inf
        } else {
            return vec3(0.0, 1.0, 1.0); // Cyan for -Inf
        }
    } else if (t < 0.0) {
        return vec3(0.0, 0.0, 1.0); // Green for negative values
    } else if (t > 1.0) {
        return vec3(1.0, 0.0, 0.0); // Red for values greater than 1
    } else {
        return vec3(t, t, t); // Black for valid numbers
    }
}