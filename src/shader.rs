use egui_glow::glow; // Re-exported glow
use glow::HasContext;
use color_eyre::eyre::{Result, eyre};

pub struct ImageProgram {
    pub program: glow::Program,
    pub vao: glow::VertexArray,
    pub vbo: glow::Buffer,
    pub ebo: glow::Buffer,
    u_tex: Option<glow::UniformLocation>,
    u_gray: Option<glow::UniformLocation>,
}

impl ImageProgram {
    pub fn new(gl: &glow::Context) -> Result<Self> {
        unsafe {
            let vert_src = r#"#version 330 core
                layout (location = 0) in vec2 aPos;
                layout (location = 1) in vec2 aUv;
                out vec2 vUv;
                void main(){
                    vUv = aUv;
                    gl_Position = vec4(aPos, 0.0, 1.0);
                }
            "#;
            let frag_src = r#"#version 330 core
                out vec4 FragColor;
                in vec2 vUv;
                uniform sampler2D u_tex;
                uniform int u_gray;
                void main(){
                    vec4 c = texture(u_tex, vUv);
                    if(u_gray == 1){
                        float g = dot(c.rgb, vec3(0.299,0.587,0.114));
                        FragColor = vec4(vec3(g), c.a);
                    } else {
                        // simple slight contrast curve example
                        vec3 rgb = c.rgb;
                        rgb = pow(rgb, vec3(1.0/1.1));
                        FragColor = vec4(rgb, c.a);
                    }
                }
            "#;
            let program = gl.create_program().map_err(|e| eyre!(e))?;
            let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
            gl.shader_source(vs, vert_src);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) { return Err(eyre!(gl.get_shader_info_log(vs))); }
            let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
            gl.shader_source(fs, frag_src);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) { return Err(eyre!(gl.get_shader_info_log(fs))); }
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            if !gl.get_program_link_status(program) { return Err(eyre!(gl.get_program_info_log(program))); }
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);

            let vertices: [f32; 16] = [
                // pos   // uv
                -1.0, -1.0, 0.0, 0.0,
                 1.0, -1.0, 1.0, 0.0,
                 1.0,  1.0, 1.0, 1.0,
                -1.0,  1.0, 0.0, 1.0,
            ];
            let indices: [u32; 6] = [0,1,2, 2,3,0];

            let vao = gl.create_vertex_array().unwrap();
            let vbo = gl.create_buffer().unwrap();
            let ebo = gl.create_buffer().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&vertices), glow::STATIC_DRAW);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytemuck::cast_slice(&indices), glow::STATIC_DRAW);
            let stride = (4 * std::mem::size_of::<f32>()) as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 2 * 4);
            gl.bind_vertex_array(None);

            let u_tex = gl.get_uniform_location(program, "u_tex");
            let u_gray = gl.get_uniform_location(program, "u_gray");

            Ok(Self { program, vao, vbo, ebo, u_tex, u_gray })
        }
    }

    pub unsafe fn draw(&self, gl: &glow::Context, tex_id: glow::NativeTexture, grayscale: bool) {
        gl.use_program(Some(self.program));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex_id));
        if let Some(loc) = &self.u_tex { gl.uniform_1_i32(Some(loc), 0); }
        if let Some(loc) = &self.u_gray { gl.uniform_1_i32(Some(loc), if grayscale {1} else {0}); }
        gl.bind_vertex_array(Some(self.vao));
        gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_INT, 0);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }
}

impl Drop for ImageProgram {
    fn drop(&mut self) {
        // Usually cleaned up automatically when context is destroyed; skip for brevity.
    }
}
