use color_eyre::eyre::{eyre, Result};

pub struct ShaderBuilder {
    base_rgb_shader: String,
    base_mono_shader: String,
}

const COLOR_PROCESS_RGB_COLORMAP: &str = r#"
    vec3 v;
    v.r = color_proc(tex.r);
    v.g = color_proc(tex.g);
    v.b = color_proc(tex.b);
    vec3 cm = colormap(v);
"#;

const COLOR_PROCESS_MONO_COLORMAP: &str = r#"
    float v = color_proc(tex.r);
    vec3 cm = colormap(v);
"#;

const IMAGE_SHADER_CODE: &str = include_str!("image_shader.frag");

impl ShaderBuilder {
    pub fn new() -> Self {
        let base_rgb_shader = IMAGE_SHADER_CODE.replace("%color_process%", COLOR_PROCESS_RGB_COLORMAP);
        let base_mono_shader = IMAGE_SHADER_CODE.replace("%color_process%", COLOR_PROCESS_MONO_COLORMAP);
        Self {
            base_rgb_shader,
            base_mono_shader,
        }
    }

    pub fn build(&self, colormap_name: &str, is_mono: bool) -> Result<String> {
        let base_dir = crate::util::path_ext::exe_dir_or_cwd();
        let path = if is_mono {
            base_dir.join(format!("colormap/mono/{}.glsl", colormap_name))
        } else {
            base_dir.join(format!("colormap/rgb/{}.glsl", colormap_name))
        };
        let colormap_code = std::fs::read_to_string(&path)
            .map_err(|e| eyre!("Failed to read colormap file '{}': {e}", path.display()))?;

        let shader = if is_mono {
            self.base_mono_shader.replace("%colormap_function%", &colormap_code)
        } else {
            self.base_rgb_shader.replace("%colormap_function%", &colormap_code)
        };
        Ok(shader)
    }
}
