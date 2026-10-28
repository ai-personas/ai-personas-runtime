//! Solid geometry preview for produced models. No CAD scripts are executed.
use eframe::{
    egui,
    glow::{self, HasContext},
};
use glam::{Mat4, Vec3};
use std::{
    io::Cursor,
    path::Path,
    sync::{Arc, Mutex},
};

pub struct Mesh {
    vertices: Vec<f32>, // position, normal, material color
    pub triangles: usize,
    pub extent: [f32; 3],
}
const MAX_TRIANGLES: usize = 1_000_000;

impl Mesh {
    fn triangle(&mut self, p: [Vec3; 3], color: [f32; 3]) -> Result<(), String> {
        if self.triangles >= MAX_TRIANGLES {
            return Err("This model exceeds the one-million-triangle preview limit.".into());
        }
        if p.iter().any(|p| !p.is_finite()) {
            return Err("The model contains non-finite coordinates.".into());
        }
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for point in p {
            self.vertices.extend_from_slice(&point.to_array());
            self.vertices.extend_from_slice(&n.to_array());
            self.vertices.extend_from_slice(&color);
        }
        self.triangles += 1;
        Ok(())
    }
    fn normalize(&mut self) -> Result<(), String> {
        if self.triangles == 0 {
            return Err("No triangle geometry was found in this model.".into());
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in self.vertices.chunks_exact(9) {
            let p = Vec3::new(v[0], v[1], v[2]);
            min = min.min(p);
            max = max.max(p);
        }
        let extent = max - min;
        self.extent = extent.to_array();
        let scale = 2.0 / extent.max_element().max(1e-9);
        let center = min + extent * 0.5;
        for v in self.vertices.chunks_exact_mut(9) {
            let p = (Vec3::new(v[0], v[1], v[2]) - center) * scale;
            v[..3].copy_from_slice(&p.to_array());
        }
        Ok(())
    }
}

pub fn load(bytes: &[u8], extension: &str, path: Option<&Path>) -> Result<Arc<Mesh>, String> {
    let mut mesh = Mesh {
        vertices: Vec::new(),
        triangles: 0,
        extent: [0.0; 3],
    };
    let solid = [0.58, 0.64, 0.72];
    match extension {
        "stl" => {
            let stl = stl_io::read_stl(&mut Cursor::new(bytes)).map_err(|e| e.to_string())?;
            for f in stl.faces {
                mesh.triangle(
                    f.vertices.map(|i| Vec3::from_array(stl.vertices[i].into())),
                    solid,
                )?;
            }
        }
        "obj" => {
            let (objects, _) = tobj::load_obj_buf(
                &mut Cursor::new(bytes),
                &tobj::LoadOptions {
                    triangulate: true,
                    single_index: true,
                    ..Default::default()
                },
                |_| Ok((Vec::new(), Default::default())),
            )
            .map_err(|e| e.to_string())?;
            for object in objects {
                for f in object.mesh.indices.chunks_exact(3) {
                    let mut p = [Vec3::ZERO; 3];
                    for (n, i) in f.iter().enumerate() {
                        let v = object
                            .mesh
                            .positions
                            .get(*i as usize * 3..*i as usize * 3 + 3)
                            .ok_or("Invalid OBJ vertex index")?;
                        p[n] = Vec3::new(v[0], v[1], v[2]);
                    }
                    mesh.triangle(p, solid)?;
                }
            }
        }
        "glb" | "gltf" => {
            use base64::Engine;
            let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
            let mut buffers = Vec::new();
            for buffer in gltf.buffers() {
                let data = match buffer.source() {
                    gltf::buffer::Source::Bin => {
                        gltf.blob.clone().ok_or("Missing GLB binary buffer")?
                    }
                    gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => {
                        let (mime, body) =
                            uri.split_once(',').ok_or("Invalid embedded glTF buffer")?;
                        if !mime.ends_with(";base64") {
                            return Err("Only base64 glTF data buffers can be previewed.".into());
                        }
                        base64::engine::general_purpose::STANDARD
                            .decode(body)
                            .map_err(|e| e.to_string())?
                    }
                    gltf::buffer::Source::Uri(uri) => {
                        if uri.contains(":") {
                            return Err(
                                "Remote glTF buffers are not loaded. Use a self-contained GLB."
                                    .into(),
                            );
                        }
                        let base = path.and_then(Path::parent).ok_or("This glTF has external buffers. Preview a self-contained GLB or the extracted model.")?;
                        super::artifacts::read_file(&base.join(uri))?
                    }
                };
                buffers.push(data);
            }
            fn node(
                n: gltf::Node<'_>,
                parent: Mat4,
                buffers: &[Vec<u8>],
                out: &mut Mesh,
                depth: usize,
            ) -> Result<(), String> {
                if depth > 128 {
                    return Err("The glTF scene hierarchy is too deep to preview.".into());
                }
                let transform = parent * Mat4::from_cols_array_2d(&n.transform().matrix());
                if let Some(m) = n.mesh() {
                    for primitive in m.primitives() {
                        let reader =
                            primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                        let positions: Vec<_> = reader
                            .read_positions()
                            .ok_or("Missing glTF positions")?
                            .map(|p| transform.transform_point3(Vec3::from_array(p)))
                            .collect();
                        let indices: Vec<u32> = reader
                            .read_indices()
                            .map(|i| i.into_u32().collect())
                            .unwrap_or_else(|| (0..positions.len() as u32).collect());
                        let c = primitive
                            .material()
                            .pbr_metallic_roughness()
                            .base_color_factor();
                        let mut add = |ids: [u32; 3]| -> Result<(), String> {
                            let mut p = [Vec3::ZERO; 3];
                            for (n, i) in ids.iter().enumerate() {
                                p[n] = *positions
                                    .get(*i as usize)
                                    .ok_or("Invalid glTF vertex index")?;
                            }
                            out.triangle(p, [c[0], c[1], c[2]])
                        };
                        match primitive.mode() {
                            gltf::mesh::Mode::Triangles => {
                                for f in indices.chunks_exact(3) {
                                    add([f[0], f[1], f[2]])?;
                                }
                            }
                            gltf::mesh::Mode::TriangleStrip => {
                                for i in 2..indices.len() {
                                    if i % 2 == 0 {
                                        add([indices[i - 2], indices[i - 1], indices[i]])?;
                                    } else {
                                        add([indices[i - 1], indices[i - 2], indices[i]])?;
                                    }
                                }
                            }
                            gltf::mesh::Mode::TriangleFan => {
                                for i in 2..indices.len() {
                                    add([indices[0], indices[i - 1], indices[i]])?;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                for child in n.children() {
                    node(child, transform, buffers, out, depth + 1)?;
                }
                Ok(())
            }
            // glTF is Y-up; the CAD viewport uses Z-up.
            let orientation = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
            let scene = gltf
                .default_scene()
                .or_else(|| gltf.scenes().next())
                .ok_or("No glTF scene")?;
            for n in scene.nodes() {
                node(n, orientation, &buffers, &mut mesh, 0)?;
            }
        }
        _ => return Err("Unsupported mesh format".into()),
    }
    mesh.normalize()?;
    Ok(Arc::new(mesh))
}

pub struct Viewport {
    gpu: Option<Arc<Mutex<Gpu>>>,
    pub error: Option<String>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: Vec3,
    wire: bool,
    grid: bool,
}
impl Viewport {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let (gpu, error) = match gl {
            Some(gl) => match unsafe { Gpu::new(gl.clone()) } {
                Ok(g) => (Some(Arc::new(Mutex::new(g))), None),
                Err(e) => (None, Some(e)),
            },
            None => (
                None,
                Some("OpenGL is unavailable for model previews.".into()),
            ),
        };
        Self {
            gpu,
            error,
            yaw: -0.8,
            pitch: 0.55,
            distance: 4.8,
            target: Vec3::ZERO,
            wire: false,
            grid: true,
        }
    }
    pub fn reset(&mut self) {
        self.yaw = -0.8;
        self.pitch = 0.55;
        self.distance = 4.8;
        self.target = Vec3::ZERO;
    }
    pub fn destroy(&mut self) {
        if let Some(g) = self.gpu.take() {
            unsafe {
                g.lock().unwrap().destroy();
            }
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, mesh: &Arc<Mesh>) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Fit model").clicked() {
                self.reset();
            }
            ui.checkbox(&mut self.wire, "Wireframe");
            ui.checkbox(&mut self.grid, "Grid");
            ui.weak(format!("{} triangles", mesh.triangles));
        });
        if let Some(e) = &self.error {
            ui.label(e);
            return;
        }
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(80.0, 80.0)),
            egui::Sense::click_and_drag(),
        );
        ui.painter()
            .rect_filled(rect, 0, egui::Color32::from_rgb(56, 57, 60));
        let delta = ui.input(|i| i.pointer.delta());
        let pan = response.dragged_by(egui::PointerButton::Secondary)
            || (response.dragged() && ui.input(|i| i.modifiers.shift));
        let eye_dir = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
        );
        let right = Vec3::Z.cross(eye_dir).normalize();
        let up = eye_dir.cross(right).normalize();
        if pan {
            self.target += (-right * delta.x + up * delta.y) * self.distance * 0.0015;
        } else if response.dragged() {
            self.yaw -= delta.x * 0.009;
            self.pitch = (self.pitch + delta.y * 0.009).clamp(-1.5, 1.5);
        }
        if response.hovered() {
            self.distance = (self.distance
                * (-ui.input(|i| i.smooth_scroll_delta.y) * 0.003).exp())
            .clamp(0.3, 100.0);
        }
        if response.double_clicked() {
            self.reset();
        }
        let eye = self.target
            + Vec3::new(
                self.yaw.cos() * self.pitch.cos(),
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
            ) * self.distance;
        let vp = Mat4::perspective_rh_gl(45.0_f32.to_radians(), rect.aspect_ratio(), 0.01, 300.0)
            * Mat4::look_at_rh(eye, self.target, Vec3::Z);
        if let Some(gpu) = &self.gpu {
            let gpu = gpu.clone();
            let mesh = mesh.clone();
            let wire = self.wire;
            let grid = self.grid;
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(mut gpu) = gpu.lock() {
                        unsafe {
                            gpu.paint(&mesh, &vp, wire, grid);
                        }
                    }
                })),
            });
        }
        ui.painter().text(
            rect.left_bottom() + egui::vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            "Drag: orbit   Shift-drag / right-drag: pan   Scroll: zoom",
            egui::FontId::proportional(10.5),
            egui::Color32::from_gray(200),
        );
        let center = rect.right_top() + egui::vec2(-36.0, 36.0);
        for (axis, label, color) in [
            (Vec3::X, "X", egui::Color32::from_rgb(224, 112, 107)),
            (Vec3::Y, "Y", egui::Color32::from_rgb(132, 191, 106)),
            (Vec3::Z, "Z", egui::Color32::from_rgb(111, 157, 224)),
        ] {
            let end = center + egui::vec2(axis.dot(right), -axis.dot(up)) * 22.0;
            ui.painter()
                .line_segment([center, end], egui::Stroke::new(1.5_f32, color));
            ui.painter().text(
                end,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(10.0),
                color,
            );
        }
    }
}

struct Gpu {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vao: glow::VertexArray,
    buffer: glow::Buffer,
    edges: glow::Buffer,
    grid_buffer: glow::Buffer,
    mesh: Option<Arc<Mesh>>,
    grid_count: i32,
}
impl Gpu {
    unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let version = if gl.version().is_embedded {
            "#version 300 es\nprecision mediump float;\n"
        } else {
            "#version 150\n"
        };
        let program = gl.create_program()?;
        let sources=[(glow::VERTEX_SHADER,"in vec3 position; in vec3 normal; in vec3 color; uniform mat4 matrix; out vec3 n; out vec3 c; void main(){gl_Position=matrix*vec4(position,1.0);n=normal;c=color;}"),
            (glow::FRAGMENT_SHADER,"in vec3 n; in vec3 c; uniform bool lit; out vec4 frag; void main(){float light=lit ? 0.30+0.70*abs(dot(normalize(n),normalize(vec3(0.4,-0.6,0.8)))):1.0;frag=vec4(c*light,1.0);}")];
        for (kind, source) in sources {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, &format!("{version}{source}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let error = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                gl.delete_program(program);
                return Err(error);
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        for (i, name) in ["position", "normal", "color"].iter().enumerate() {
            gl.bind_attrib_location(program, i as u32, name);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(error);
        }
        let vao = gl.create_vertex_array()?;
        let buffer = gl.create_buffer()?;
        let edges = gl.create_buffer()?;
        let grid_buffer = gl.create_buffer()?;
        gl.bind_vertex_array(Some(vao));
        let mut grid = Vec::new();
        for i in -12..=12 {
            let f = i as f32 * 0.25;
            for p in [
                [f, -3.0, -1.02],
                [f, 3.0, -1.02],
                [-3.0, f, -1.02],
                [3.0, f, -1.02],
            ] {
                grid.extend_from_slice(&p);
                grid.extend_from_slice(&[0.0, 0.0, 1.0]);
                grid.extend_from_slice(if i == 0 {
                    &[0.42, 0.43, 0.46]
                } else {
                    &[0.29, 0.30, 0.32]
                });
            }
        }
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(grid_buffer));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &float_bytes(&grid), glow::STATIC_DRAW);
        gl.bind_vertex_array(None);
        Ok(Self {
            gl,
            program,
            vao,
            buffer,
            edges,
            grid_buffer,
            mesh: None,
            grid_count: (grid.len() / 9) as i32,
        })
    }
    unsafe fn attributes(&self, buffer: glow::Buffer) {
        let gl = &self.gl;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
        for i in 0..3 {
            gl.enable_vertex_attrib_array(i);
            gl.vertex_attrib_pointer_f32(i, 3, glow::FLOAT, false, 36, (i * 12) as i32);
        }
    }
    unsafe fn paint(&mut self, mesh: &Arc<Mesh>, matrix: &Mat4, wire: bool, grid: bool) {
        let gl = &self.gl;
        gl.bind_vertex_array(Some(self.vao));
        if self.mesh.as_ref().is_none_or(|old| !Arc::ptr_eq(old, mesh)) {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffer));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                &float_bytes(&mesh.vertices),
                glow::STATIC_DRAW,
            );
            let mut edges = Vec::with_capacity(mesh.triangles * 6 * 4);
            for i in 0..mesh.triangles as u32 {
                for n in [i * 3, i * 3 + 1, i * 3 + 1, i * 3 + 2, i * 3 + 2, i * 3] {
                    edges.extend_from_slice(&n.to_ne_bytes());
                }
            }
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.edges));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, &edges, glow::STATIC_DRAW);
            self.mesh = Some(mesh.clone());
        }
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.use_program(Some(self.program));
        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(self.program, "matrix").as_ref(),
            false,
            &matrix.to_cols_array(),
        );
        let lit = gl.get_uniform_location(self.program, "lit");
        if grid {
            gl.uniform_1_i32(lit.as_ref(), 0);
            self.attributes(self.grid_buffer);
            gl.draw_arrays(glow::LINES, 0, self.grid_count);
        }
        self.attributes(self.buffer);
        gl.uniform_1_i32(lit.as_ref(), if wire { 0 } else { 1 });
        if wire {
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.edges));
            gl.draw_elements(
                glow::LINES,
                (mesh.triangles * 6) as i32,
                glow::UNSIGNED_INT,
                0,
            );
        } else {
            gl.draw_arrays(glow::TRIANGLES, 0, (mesh.triangles * 3) as i32);
        }
        gl.disable(glow::DEPTH_TEST);
        gl.depth_mask(false);
        gl.bind_vertex_array(None);
    }
    unsafe fn destroy(&mut self) {
        self.gl.delete_buffer(self.buffer);
        self.gl.delete_buffer(self.edges);
        self.gl.delete_buffer(self.grid_buffer);
        self.gl.delete_vertex_array(self.vao);
        self.gl.delete_program(self.program);
    }
}
fn float_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}
