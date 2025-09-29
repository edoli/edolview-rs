#version 330 core

out vec4 frag_color;

in vec2 v_tex_coord;

uniform vec2 u_image_size;

uniform sampler2D u_texture;
uniform int u_channel_index;

uniform int u_use_alpha; // 0: off, 1: on
uniform float u_offset;
uniform float u_exposure;
uniform float u_gamma;
uniform float u_min_v;
uniform float u_max_v;
uniform int u_scale_mode; // 0: linear, 1: inverse, 2: log

// Per-channel controls
uniform int u_use_per_channel; // 0: global, 1: per-channel
uniform float u_min_v0;
uniform float u_min_v1;
uniform float u_min_v2;
uniform float u_min_v3;
uniform float u_max_v0;
uniform float u_max_v1;
uniform float u_max_v2;
uniform float u_max_v3;
uniform int u_scale_mode0;
uniform int u_scale_mode1;
uniform int u_scale_mode2;
uniform int u_scale_mode3;

#define PI 3.1415926535897932384626433832795
#define EPS 1e-12

mat3 rgb2xyz_mat = mat3(
    0.4124564, 0.3575761, 0.1804375,
    0.2126729, 0.7151522, 0.0721750,
    0.0193339, 0.1191920, 0.9503041
);

mat3 xyz2rgb_mat = mat3(
    3.2404542, -1.5371385, -0.4985314,
    -0.9692660, 1.8760108, 0.0415560,
    0.0556434, -0.2040259, 1.0572252
);

// d65 white reference
float xn = 0.95047;
float yn = 1.0;
float zn = 1.08883;

vec3 gamma_f(vec3 rgb) {
    return vec3(
        pow(rgb.r, 1.0 / 2.2),
        pow(rgb.g, 1.0 / 2.2),
        pow(rgb.b, 1.0 / 2.2)
    );
}

vec3 inv_gamma_f(vec3 rgb) {
    return vec3(
        pow(rgb.r, 2.2),
        pow(rgb.g, 2.2),
        pow(rgb.b, 2.2)
    );
}

float f(float c) {
    return c > 0.008856 ? pow(c, 1.0 / 3.0) : (903.3 * c + 16.0) / 116.0;
}

vec3 xyz2lab(vec3 xyz){
    float fx = f(xyz.x / xn);
    float fy = f(xyz.y / yn);
    float fz = f(xyz.z / zn);
    
    return vec3(
        116.0 * fx - 16.0,
        500.0 * (fx - fy),
        200.0 * (fy - fz)
    );
}

float f_inv(float c) {
    float t = pow(c, 3.0);
    return t > 0.008856 ? t : (116.0 * c - 16.0) / 903.3;
}

vec3 lab2xyz(vec3 lab){
    float L = lab.x;
    float a = lab.y;
    float b = lab.z;

    float fy = (L + 16.0) / 116.0;
    float fz = fy - b / 200.0;
    float fx = a / 500.0 + fy;

    return vec3(
        f_inv(fx) * xn, 
        f_inv(fy) * yn, 
        f_inv(fz) * zn
    );
}

vec3 rgb2xyz(vec3 rgb) {
    return inv_gamma_f(rgb) * rgb2xyz_mat;
}

vec3 xyz2rgb(vec3 xyz) {
    return gamma_f(xyz * xyz2rgb_mat);
}

vec3 lab2rgb(vec3 lab) {
    return xyz2rgb(lab2xyz(lab));
}

vec3 rgb2lab(vec3 rgb) {
    return xyz2lab(rgb2xyz(rgb));
}

float color_proc(float v)
{
    float tmp = v * exp2(u_exposure) + u_offset;
    return sign(tmp) * pow(abs(tmp), 1.0 / u_gamma);
}

float apply_scale_mode(float v, int mode)
{
    if (mode == 1) {
        // Inverse: 1 / v
        float s = sign(v);
        return s / (abs(v) + EPS);
    } else if (mode == 2) {
        // Log: log(v)
        float s = sign(v);
        return s * log(abs(v) + 1.0);
    } else {
        // Linear: v
        return v;
    }
}

%colormap_function%

void main()
{
    vec4 tex = texture2D(u_texture, v_tex_coord);

    float image_x = v_tex_coord.x * float(u_image_size.x);
    float image_y = v_tex_coord.y * float(u_image_size.y);
    
    if (u_use_per_channel != 0) {
        tex.r = (apply_scale_mode(tex.r, u_scale_mode0) - u_min_v0) / (u_max_v0 - u_min_v0);
        tex.g = (apply_scale_mode(tex.g, u_scale_mode1) - u_min_v1) / (u_max_v1 - u_min_v1);
        tex.b = (apply_scale_mode(tex.b, u_scale_mode2) - u_min_v2) / (u_max_v2 - u_min_v2);
        tex.a = (apply_scale_mode(tex.a, u_scale_mode3) - u_min_v3) / (u_max_v3 - u_min_v3);
    } else {
        tex.r = (apply_scale_mode(tex.r, u_scale_mode) - u_min_v) / (u_max_v - u_min_v);
        tex.g = (apply_scale_mode(tex.g, u_scale_mode) - u_min_v) / (u_max_v - u_min_v);
        tex.b = (apply_scale_mode(tex.b, u_scale_mode) - u_min_v) / (u_max_v - u_min_v);
    }

    if (u_channel_index == 1) {
        tex.r = tex.g;
    } else if (u_channel_index == 2) {
        tex.r = tex.b;
    } else if (u_channel_index == 3) {
        tex.r = tex.a;
    }

    float alpha;
    if (u_channel_index != -1 || u_use_alpha == 0) {
        alpha = 1.0;
    } else {
        alpha = tex.a;
    }

    %color_process%

    frag_color = vec4(cm.r, cm.g, cm.b, alpha);
}