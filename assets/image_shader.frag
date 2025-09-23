#version 330 core

out vec4 frag_color;

in vec2 v_tex_coord;

uniform vec2 u_image_size;

uniform sampler2D u_texture;
uniform int u_channel_index;

uniform float u_offset;
uniform float u_exposure;
uniform float u_gamma;
uniform float u_min_v;
uniform float u_max_v;
uniform int u_scale_mode; // 0: linear, 1: inverse, 2: log
uniform float u_r_scale;
uniform float u_g_scale;
uniform float u_b_scale;

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

%colormap_function%

void main()
{
    vec4 tex = texture2D(u_texture, v_tex_coord);
    float alpha = tex.a;

    tex.r *= u_r_scale;
    tex.g *= u_g_scale;
    tex.b *= u_b_scale;

    float image_x = v_tex_coord.x * float(u_image_size.x);
    float image_y = v_tex_coord.y * float(u_image_size.y);

    if (u_scale_mode == 1) {
        // Inverse: 1 / v
        vec4 s = sign(tex);
        tex = s / (abs(tex) + vec4(EPS));
    } else if (u_scale_mode == 2) {
        // Log: log(v)
        vec4 s = sign(tex);
        tex = s * log(abs(tex) + 1);
    } else {
        // Linear: v
    }

    tex = (tex - u_min_v) / (u_max_v - u_min_v);

    if (u_channel_index == 1) {
        tex.r = tex.g;
    } else if (u_channel_index == 2) {
        tex.r = tex.b;
    } else if (u_channel_index == 3) {
        tex.r = alpha;
    }

    if (u_channel_index != -1) {
        alpha = 1.0;
    }

    %color_process%

    frag_color = vec4(cm.r, cm.g, cm.b, alpha);
}