//! The 3D view's picture drawn on the graphics card: the same scene as `terrain::render` (the
//! ground and its pictures, the blocks, the line with its edging and its hidden part), read back
//! as bytes for the window to show as an image (the UI toolkit draws no 3D of its own). Each
//! picture is drawn whole and sharp (4 samples per pixel) in a few milliseconds.
//!
//! `render` gives `None` when there is no usable card or it failed once: the caller then draws
//! on the processor.

use crate::logic::terrain::{Camera, Face, HIDDEN_LINE, Inset, LINE_BIAS, NEAR, Terrain, Texture};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use wgpu::util::DeviceExt;

const SAMPLES: u32 = 4;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
/// Finer pictures over the ground, at most (the 3D view makes four levels).
const INSETS: usize = 4;

/// The device, what draws, and what was uploaded for the last pictures.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    ground: wgpu::RenderPipeline,
    solid: wgpu::RenderPipeline,
    edging: wgpu::RenderPipeline,
    hidden: wgpu::RenderPipeline,
    seen: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    /// A 1 × 1 picture in the inset slots not used.
    blank: wgpu::TextureView,
    max_size: u32,
    state: Mutex<State>,
}

/// Uploaded once and kept while the same `Arc` comes back (it is held, so its address is not
/// reused for another ground).
#[derive(Default)]
struct State {
    mesh: Option<(Arc<Terrain>, wgpu::Buffer, wgpu::Buffer, u32)>,
    texture: Option<(Arc<Texture>, wgpu::TextureView)>,
    insets: Vec<(Arc<Inset>, wgpu::TextureView)>,
    target: Option<Target>,
}

/// For a size: the drawing's samples and depth, the resolved picture, and where it is read.
type Target = ((u32, u32), wgpu::TextureView, wgpu::TextureView, wgpu::Texture, wgpu::Buffer);

/// Set by the device's error handler: from then on the processor draws.
static FAILED: AtomicBool = AtomicBool::new(false);

fn gpu() -> Option<&'static Gpu> {
    static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
    GPU.get_or_init(|| {
        let g = Gpu::new();
        if let Err(e) = &g {
            eprintln!("3D on the processor: {e}");
        }
        g.ok()
    })
    .as_ref()
    .filter(|_| !FAILED.load(Ordering::Relaxed))
}

/// The card's requests are answered at once on the desktop: polled until ready.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_ne_bytes()).collect()
}

/// Draws as `terrain::render` does (with `smooth`), `w` × `h` pixels of 4 bytes, the colour bytes
/// as given, alpha 255. `None`: draw on the processor.
#[allow(clippy::too_many_arguments)]
pub fn render(t: &Arc<Terrain>, texture: &Arc<Texture>, solids: &[Face], cam: &Camera, exaggeration: f32, line: &[([f64; 3], [u8; 3])], w: usize, h: usize, background: [u8; 4], insets: &[Arc<Inset>]) -> Option<Vec<u8>> {
    let g = gpu()?;
    let fits = |a: usize, b: usize| a >= 2 && b >= 2 && a <= g.max_size as usize && b <= g.max_size as usize;
    if !fits(w, h) || !fits(texture.w, texture.h) || texture.px.len() < texture.w * texture.h || t.cols < 2 || t.rows < 2 || insets.iter().any(|i| !fits(i.texture.w, i.texture.h)) {
        return None;
    }
    let px = g.draw(t, texture, solids, cam, exaggeration, line, w as u32, h as u32, background, &insets[..insets.len().min(INSETS)]);
    px.filter(|_| !FAILED.load(Ordering::Relaxed))
}

impl Gpu {
    fn new() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::LowPower, compatible_surface: None, force_fallback_adapter: false })).map_err(|e| e.to_string())?;
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("terrain"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()).using_alignment(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| e.to_string())?;
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
            eprintln!("3D on the graphics card failed, now on the processor: {e}");
            FAILED.store(true, Ordering::Relaxed);
        }));
        let max_size = device.limits().max_texture_dimension_2d;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("terrain"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ];
        entries.extend((2..3 + INSETS as u32).map(texture_entry));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("terrain"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("terrain"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let stencil_once = wgpu::StencilFaceState { compare: wgpu::CompareFunction::Equal, fail_op: wgpu::StencilOperation::Keep, depth_fail_op: wgpu::StencilOperation::Keep, pass_op: wgpu::StencilOperation::IncrementClamp };
        // (vertex entry, fragment entry, buffer, write depth, depth test, cull, blend, once per sample, coverage from alpha)
        let pipeline = |vs: &str, fs: &str, buffer: wgpu::VertexBufferLayout, write: bool, test: wgpu::CompareFunction, cull: bool, blend: bool, once: bool, coverage: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers: &[buffer] },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // Seen from outside, a face goes round clockwise on screen with y down: counter-clockwise with y up.
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: cull.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(write),
                    depth_compare: Some(test),
                    stencil: if once { wgpu::StencilState { front: stencil_once, back: stencil_once, read_mask: 0xff, write_mask: 0xff } } else { wgpu::StencilState::default() },
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: coverage },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOUR,
                        blend: blend.then_some(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::REPLACE }),
                        // The picture stays opaque: the line's soft edges are in the colour only.
                        write_mask: wgpu::ColorWrites::COLOR,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let buffer = |stride: u64, step: wgpu::VertexStepMode, attributes: &'static [wgpu::VertexAttribute]| wgpu::VertexBufferLayout { array_stride: stride, step_mode: step, attributes };
        const GROUND: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2];
        const SOLID: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        const LINE: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
        let (ground_attrs, solid_attrs, line_attrs): (&'static [_], &'static [_], &'static [_]) = (&GROUND, &SOLID, &LINE);
        let (vertex, instance) = (wgpu::VertexStepMode::Vertex, wgpu::VertexStepMode::Instance);
        use wgpu::CompareFunction::{Greater, LessEqual};
        // Depth is 1 / distance: nearer is greater.
        let ground = pipeline("vs_ground", "fs_ground", buffer(20, vertex, ground_attrs), true, Greater, false, false, false, false);
        let solid = pipeline("vs_solid", "fs_solid", buffer(16, vertex, solid_attrs), true, Greater, true, false, false, false);
        // The edging where the line is in sight, without depth of its own; then what is hidden of
        // the line, faint and once per sample; then the line in sight.
        let edging = pipeline("vs_line", "fs_edging", buffer(28, instance, line_attrs), false, Greater, false, false, false, true);
        let hidden = pipeline("vs_line", "fs_hidden", buffer(28, instance, line_attrs), false, LessEqual, false, true, true, false);
        let seen = pipeline("vs_line", "fs_seen", buffer(28, instance, line_attrs), true, Greater, false, false, false, true);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: Some("view"), size: UNIFORM_FLOATS as u64 * 4, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let blank = upload(&device, &queue, &Texture { w: 1, h: 1, px: vec![[0; 3]], fade: 0.0 });
        Ok(Gpu { device, queue, layout, ground, solid, edging, hidden, seen, sampler, uniforms, blank, max_size, state: Mutex::new(State::default()) })
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&self, t: &Arc<Terrain>, texture: &Arc<Texture>, solids: &[Face], cam: &Camera, exaggeration: f32, line: &[([f64; 3], [u8; 3])], w: u32, h: u32, background: [u8; 4], insets: &[Arc<Inset>]) -> Option<Vec<u8>> {
        let (device, queue) = (&self.device, &self.queue);
        let mut st = self.state.lock().ok()?;
        let st = &mut *st;
        if !st.mesh.as_ref().is_some_and(|m| Arc::ptr_eq(&m.0, t)) {
            st.mesh = Some(mesh(device, t));
        }
        if !st.texture.as_ref().is_some_and(|m| Arc::ptr_eq(&m.0, texture)) {
            st.texture = Some((texture.clone(), upload(device, queue, texture)));
        }
        st.insets.retain(|(i, _)| insets.iter().any(|j| Arc::ptr_eq(i, j)));
        for i in insets {
            if !st.insets.iter().any(|(j, _)| Arc::ptr_eq(i, j)) {
                st.insets.push((i.clone(), upload(device, queue, &i.texture)));
            }
        }
        if st.target.as_ref().is_none_or(|t| t.0 != (w, h)) {
            st.target = Some(target(device, w, h));
        }
        let (Some((_, vertices, indices, count)), Some((_, ground_view)), Some((_, colour, depth, resolved, readback))) = (&st.mesh, &st.texture, &st.target) else { return None };

        // The eye, its axes and its scales to the picture, the line's widths (as the processor
        // draws them on twice the pixels: in pixels here), the background, the pictures' parts.
        let ex = exaggeration as f64;
        let (eye, b) = cam.basis(ex);
        let tan = (cam.fov / 2.0).tan();
        let radius = (w as f32 * 3.0 / 1200.0).max(0.5);
        let edging = radius + (radius * 0.4).max(0.5);
        let mut u = Vec::with_capacity(UNIFORM_FLOATS);
        u.extend(eye.map(|v| v as f32));
        u.push(exaggeration);
        u.extend(b[0].map(|v| v as f32));
        u.push((h as f64 / w as f64 / tan) as f32);
        u.extend(b[1].map(|v| v as f32));
        u.push((1.0 / tan) as f32);
        u.extend(b[2].map(|v| v as f32));
        u.push(NEAR as f32);
        u.extend([w as f32, h as f32, radius, edging]);
        u.extend(background.map(|v| v as f32 / 255.0));
        u.extend([texture.fade, insets.len() as f32, HIDDEN_LINE, LINE_BIAS]);
        for k in 0..INSETS {
            u.extend(insets.get(k).map_or([0.0; 4], |i| i.uv));
        }
        queue.write_buffer(&self.uniforms, 0, &floats(&u));

        let mut views = vec![wgpu::BindingResource::TextureView(ground_view)];
        views.extend((0..INSETS).map(|k| wgpu::BindingResource::TextureView(insets.get(k).and_then(|i| st.insets.iter().find(|(j, _)| Arc::ptr_eq(i, j))).map_or(&self.blank, |(_, v)| v))));
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }];
        entries.extend(views.into_iter().enumerate().map(|(k, resource)| wgpu::BindGroupEntry { binding: 2 + k as u32, resource }));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("terrain"), layout: &self.layout, entries: &entries });

        // ponytail: the blocks and the line are uploaded at each picture (about 1.5 MB for 30 000
        // faces); kept on the card if that ever shows.
        let mut solid_bytes = Vec::with_capacity(solids.len() * 48);
        for f in solids.iter().filter(|f| f.p.iter().flatten().all(|v| v.is_finite())) {
            for p in f.p {
                for v in p {
                    solid_bytes.extend_from_slice(&v.to_ne_bytes());
                }
                solid_bytes.extend_from_slice(&[f.colour[0], f.colour[1], f.colour[2], 255]);
            }
        }
        // A point that is not a number cuts the line there.
        let mut line_bytes = Vec::with_capacity(line.len() * 28);
        for pair in line.windows(2) {
            let (a, b) = (pair[0].0, pair[1].0);
            if a.iter().chain(&b).all(|v| v.is_finite()) {
                for v in a.iter().chain(&b) {
                    line_bytes.extend_from_slice(&(*v as f32).to_ne_bytes());
                }
                let c = pair[0].1;
                line_bytes.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let init = |contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents, usage });
        let solid_buffer = (!solid_bytes.is_empty()).then(|| init(&solid_bytes, wgpu::BufferUsages::VERTEX));
        let line_buffer = (!line_bytes.is_empty()).then(|| init(&line_bytes, wgpu::BufferUsages::VERTEX));

        let resolved_view = resolved.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("terrain") });
        {
            let bg = background.map(|v| v as f64 / 255.0);
            let discard = wgpu::StoreOp::Discard;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terrain"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour,
                    depth_slice: None,
                    resolve_target: Some(&resolved_view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0], g: bg[1], b: bg[2], a: 1.0 }), store: discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: discard }), stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: discard }) }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &group, &[]);
            pass.set_pipeline(&self.ground);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
            if let Some(buf) = &solid_buffer {
                pass.set_pipeline(&self.solid);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..(solid_bytes.len() / 16) as u32, 0..1);
            }
            if let Some(buf) = &line_buffer {
                let n = (line_bytes.len() / 28) as u32;
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.set_stencil_reference(0);
                for p in [&self.edging, &self.hidden, &self.seen] {
                    pass.set_pipeline(p);
                    pass.draw(0..6, 0..n);
                }
            }
        }
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: resolved, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| {
            tx.send(r).ok();
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.try_recv().ok()?.ok()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        {
            let data = readback.get_mapped_range(..);
            for y in 0..h as usize {
                out.extend_from_slice(&data[y * row as usize..y * row as usize + w as usize * 4]);
            }
        }
        readback.unmap();
        Some(out)
    }
}

/// The ground's vertices (east, north, altitude in metres; place on its pictures) and triangles.
fn mesh(device: &wgpu::Device, t: &Arc<Terrain>) -> (Arc<Terrain>, wgpu::Buffer, wgpu::Buffer, u32) {
    let (cols, rows) = (t.cols, t.rows);
    let mut v = Vec::with_capacity(cols * rows * 5);
    for j in 0..rows {
        for i in 0..cols {
            v.extend([(i as f64 * t.dx) as f32, (j as f64 * t.dy) as f32, t.heights[j * cols + i], i as f32 / (cols - 1) as f32, j as f32 / (rows - 1) as f32]);
        }
    }
    let mut idx: Vec<u32> = Vec::with_capacity((cols - 1) * (rows - 1) * 6);
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            let k = (j * cols + i) as u32;
            let (k10, k01, k11) = (k + 1, k + cols as u32, k + cols as u32 + 1);
            idx.extend([k, k10, k11, k, k11, k01]);
        }
    }
    let init = |contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ground"), contents, usage });
    let bytes: Vec<u8> = idx.iter().flat_map(|k| k.to_ne_bytes()).collect();
    (t.clone(), init(&floats(&v), wgpu::BufferUsages::VERTEX), init(&bytes, wgpu::BufferUsages::INDEX), idx.len() as u32)
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, p: &Texture) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: p.w as u32, height: p.h as u32, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor { label: Some("picture"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: COLOUR, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    let bytes: Vec<u8> = p.px[..p.w * p.h].iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(p.w as u32 * 4), rows_per_image: Some(p.h as u32) },
        size,
    );
    texture.create_view(&Default::default())
}

fn target(device: &wgpu::Device, w: u32, h: u32) -> Target {
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let make = |format, samples, usage| device.create_texture(&wgpu::TextureDescriptor { label: Some("drawing"), size, mip_level_count: 1, sample_count: samples, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
    let colour = make(COLOUR, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
    let depth = make(DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
    let resolved = make(COLOUR, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor { label: Some("picture"), size: row as u64 * h as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    ((w, h), colour, depth, resolved, readback)
}

/// Seven vectors of four, then the insets' parts.
const UNIFORM_FLOATS: usize = 4 * (7 + INSETS);

const SHADER: &str = r#"
struct View {
    eye: vec4<f32>,    // the eye (scaled metres), the exaggeration of heights
    right: vec4<f32>,  // the eye's axes; w: their scales to the picture, then the near plane
    up: vec4<f32>,
    fwd: vec4<f32>,
    size: vec4<f32>,   // pixels across, down; the line's radius, its edging's
    bg: vec4<f32>,
    misc: vec4<f32>,   // the ground's fade, how many insets, the hidden line's share, the line's bias
    rects: array<vec4<f32>, 4>,
};
@group(0) @binding(0) var<uniform> u: View;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var ground: texture_2d<f32>;
@group(0) @binding(3) var fine0: texture_2d<f32>;
@group(0) @binding(4) var fine1: texture_2d<f32>;
@group(0) @binding(5) var fine2: texture_2d<f32>;
@group(0) @binding(6) var fine3: texture_2d<f32>;

fn view(p: vec3<f32>) -> vec3<f32> {
    let d = vec3<f32>(p.xy, p.z * u.eye.w) - u.eye.xyz;
    return vec3<f32>(dot(d, u.right.xyz), dot(d, u.up.xyz), dot(d, u.fwd.xyz));
}

// Depth is near / distance: 1 / distance, as on the processor.
fn clip(c: vec3<f32>) -> vec4<f32> {
    return vec4<f32>(c.x * u.right.w, c.y * u.up.w, u.fwd.w, c.z);
}

struct GroundOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_ground(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>) -> GroundOut {
    return GroundOut(clip(view(p)), uv);
}

fn edge(d: f32) -> f32 { return min(d / 0.1, 1.0); }

// An inset's colour at the ground's uv, and how much of it shows (0 outside, fading at its edges).
fn inset(t: texture_2d<f32>, k: i32, uv: vec2<f32>) -> vec4<f32> {
    let r = u.rects[k];
    let p = (uv - r.xy) / (r.zw - r.xy);
    if (any(p < vec2<f32>(0.0)) || any(p > vec2<f32>(1.0))) { return vec4<f32>(0.0); }
    let w = edge(p.x) * edge(1.0 - p.x) * edge(p.y) * edge(1.0 - p.y);
    return vec4<f32>(textureSampleLevel(t, smp, p, 0.0).rgb, w);
}

fn fade(d: f32) -> f32 {
    let t = clamp(d / u.misc.x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment fn fs_ground(i: GroundOut) -> @location(0) vec4<f32> {
    let n = i32(u.misc.y);
    var tex = vec3<f32>(0.0);
    var left = 1.0;
    // The finest picture over the coarser ones.
    if (n > 0) { let f = inset(fine0, 0, i.uv); tex += f.rgb * f.a * left; left *= 1.0 - f.a; }
    if (n > 1 && left >= 0.004) { let f = inset(fine1, 1, i.uv); tex += f.rgb * f.a * left; left *= 1.0 - f.a; }
    if (n > 2 && left >= 0.004) { let f = inset(fine2, 2, i.uv); tex += f.rgb * f.a * left; left *= 1.0 - f.a; }
    if (n > 3 && left >= 0.004) { let f = inset(fine3, 3, i.uv); tex += f.rgb * f.a * left; left *= 1.0 - f.a; }
    if (left >= 0.004) { tex += textureSampleLevel(ground, smp, i.uv, 0.0).rgb * left; }
    var shown = 1.0;
    if (u.misc.x > 0.0) { shown = fade(i.uv.x) * fade(1.0 - i.uv.x) * fade(i.uv.y) * fade(1.0 - i.uv.y); }
    return vec4<f32>(mix(u.bg.rgb, tex, shown), 1.0);
}

struct SolidOut { @builtin(position) pos: vec4<f32>, @location(0) @interpolate(flat) c: vec4<f32> };

@vertex fn vs_solid(@location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> SolidOut {
    return SolidOut(clip(view(p)), c);
}

@fragment fn fs_solid(i: SolidOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.c.rgb, 1.0);
}

// A segment of the line, as a capsule on screen: its ends (pixels, and 1 / distance).
struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) a: vec3<f32>,
    @location(1) @interpolate(flat) b: vec3<f32>,
    @location(2) @interpolate(flat) c: vec4<f32>,
};

fn screen(c: vec3<f32>) -> vec3<f32> {
    let half = u.size.xy / 2.0;
    let f = u.up.w * half.y;
    return vec3<f32>(half.x + c.x / c.z * f, half.y - c.y / c.z * f, 1.0 / c.z);
}

@vertex fn vs_line(@builtin(vertex_index) k: u32, @location(0) pa: vec3<f32>, @location(1) pb: vec3<f32>, @location(2) col: vec4<f32>) -> LineOut {
    var out: LineOut;
    out.c = col;
    var ca = view(pa);
    var cb = view(pb);
    let near = u.fwd.w;
    if (ca.z < near && cb.z < near) {
        out.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    // Cut at the near plane.
    if (ca.z < near) {
        ca = mix(ca, cb, (near - ca.z) / (cb.z - ca.z));
        ca.z = near;
    } else if (cb.z < near) {
        cb = mix(cb, ca, (near - cb.z) / (ca.z - cb.z));
        cb.z = near;
    }
    let a = screen(ca);
    let b = screen(cb);
    let len = length(b.xy - a.xy);
    var d = vec2<f32>(1.0, 0.0);
    if (len > 1e-6) { d = (b.xy - a.xy) / len; }
    let n = vec2<f32>(-d.y, d.x);
    // As wide as the edging, and a pixel more for the soft edge.
    let r = u.size.w + 1.0;
    var corners = array<vec2<f32>, 6>(vec2<f32>(0.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let q = corners[k];
    var end = a.xy - d * r;
    if (q.x > 0.5) { end = b.xy + d * r; }
    let p = end + n * r * q.y;
    out.pos = vec4<f32>(p.x / u.size.x * 2.0 - 1.0, 1.0 - p.y / u.size.y * 2.0, 0.5, 1.0);
    out.a = a;
    out.b = b;
    return out;
}

// How much of the pixel a capsule of radius r covers, and its depth there (nearer than the ground).
fn capsule(i: LineOut, r: f32) -> vec2<f32> {
    let p = i.pos.xy;
    let d = i.b.xy - i.a.xy;
    let len2 = dot(d, d);
    var t = 0.0;
    if (len2 > 1e-9) { t = clamp(dot(p - i.a.xy, d) / len2, 0.0, 1.0); }
    let e = p - i.a.xy - t * d;
    return vec2<f32>(clamp(r - length(e) + 0.5, 0.0, 1.0), min((i.a.z + t * (i.b.z - i.a.z)) * u.misc.w, 1.0));
}

struct LineFrag { @location(0) c: vec4<f32>, @builtin(frag_depth) depth: f32 };

@fragment fn fs_edging(i: LineOut) -> LineFrag {
    let c = capsule(i, u.size.w);
    if (c.x <= 0.0) { discard; }
    return LineFrag(vec4<f32>(u.bg.rgb, c.x), c.y);
}

@fragment fn fs_hidden(i: LineOut) -> LineFrag {
    let c = capsule(i, u.size.z);
    if (c.x <= 0.0) { discard; }
    return LineFrag(vec4<f32>(i.c.rgb, c.x * u.misc.z), c.y);
}

@fragment fn fs_seen(i: LineOut) -> LineFrag {
    let c = capsule(i, u.size.z);
    if (c.x <= 0.0) { discard; }
    return LineFrag(vec4<f32>(i.c.rgb, c.x), c.y);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::terrain::{self, extrude};

    fn grid(n: usize, f: impl Fn(usize, usize) -> f32) -> Arc<Terrain> {
        let heights = (0..n * n).map(|k| f(k % n, k / n)).collect();
        Arc::new(Terrain { cols: n, rows: n, heights, dx: 100.0, dy: 100.0, south: 44.0, west: 6.0, mx: 80_000.0, my: 110_540.0 })
    }

    fn gradient(n: usize, fade: f32, tint: u8) -> Texture {
        Texture { w: n, h: n, px: (0..n * n).map(|k| [(k % n * 255 / n) as u8, tint, (k / n * 255 / n) as u8]).collect(), fade }
    }

    /// Mean difference per byte, and the share of pixels off by more than 40 on a channel.
    fn compare(a: &[u8], b: &[u8]) -> (f64, f64) {
        assert_eq!(a.len(), b.len());
        let mean = a.iter().zip(b).map(|(x, y)| (*x as f64 - *y as f64).abs()).sum::<f64>() / a.len() as f64;
        let off = a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > 40)).count();
        (mean, off as f64 / (a.len() / 4) as f64)
    }

    /// The same scene as on the processor: hills under a picture fading at its edges, a finer one
    /// over part of it, blocks, and a line partly behind a ridge; a few pixels differ at edges.
    #[test]
    fn draws_what_the_processor_draws() {
        let t = grid(41, |i, j| if j == 8 { 500.0 } else { 150.0 * ((i as f32 / 6.0).sin() + (j as f32 / 5.0).cos()) });
        let texture = Arc::new(gradient(64, 0.12, 120));
        let inset = Arc::new(Inset { texture: gradient(32, 0.0, 250), uv: [0.3, 0.5, 0.6, 0.8] });
        let cam = Camera { target: [2000.0, 2200.0, 0.0], yaw: 0.3, pitch: 35f64.to_radians(), dist: 4500.0, min_dist: 300.0, fov: 0.8 };
        let solids: Vec<Face> = (0..20).flat_map(|k| {
            let (x, y) = (2200.0 + (k % 5) as f32 * 120.0, 2200.0 + (k / 5) as f32 * 120.0);
            let g = t.height_at(x as f64, y as f64) as f32;
            extrude(&[[x, y], [x + 80.0, y], [x + 80.0, y + 60.0], [x, y + 60.0]], g - 50.0, g + 150.0, [150, 140, 130], [220, 210, 200])
        }).collect();
        let mut line: Vec<([f64; 3], [u8; 3])> = (0..60).map(|k| {
            let (x, y) = (800.0 + k as f64 * 40.0, 300.0 + k as f64 * 50.0);
            ([x, y, t.height_at(x, y)], if k < 30 { [230, 60, 20] } else { [20, 60, 230] })
        }).collect();
        line[40].0[2] = f64::NAN;
        let bg = [245, 240, 235, 255];
        let (w, h) = (600, 400);
        let Some(gpu) = render(&t, &texture, &solids, &cam, 1.5, &line, w, h, bg, std::slice::from_ref(&inset)) else {
            eprintln!("no graphics card: not compared");
            return;
        };
        let cpu = terrain::render(&t, &texture, &solids, &cam, 1.5, &line, w, h, bg, true, &[&inset]);
        let (mean, off) = compare(&gpu, &cpu);
        assert!(gpu.chunks(4).all(|p| p[3] == 255), "opaque");
        assert!(mean < 3.0 && off < 0.02, "mean {mean:.2}, {:.2} % of pixels off", off * 100.0);
        // The same picture again (all kept on the card), and at another size.
        assert_eq!(render(&t, &texture, &solids, &cam, 1.5, &line, w, h, bg, std::slice::from_ref(&inset)).unwrap(), gpu);
        let small = render(&t, &texture, &solids, &cam, 1.5, &line, 301, 199, bg, &[inset]).unwrap();
        assert_eq!(small.len(), 301 * 199 * 4);
    }

    #[test]
    fn a_peak_in_front_shows_the_line_faintly_without_its_edging() {
        let t = grid(21, |_, j| if j == 5 { 1500.0 } else { 0.0 });
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: 15f64.to_radians(), dist: 4000.0, min_dist: 300.0, fov: 0.8 };
        let line = vec![([900.0, 1500.0, 0.0], [255, 0, 0]), ([1000.0, 1500.0, 0.0], [255, 0, 0]), ([1000.0, 1500.0, 0.0], [255, 0, 0]), ([1100.0, 1500.0, 0.0], [255, 0, 0])];
        let green = Arc::new(Texture { w: 2, h: 2, px: vec![[40, 160, 40]; 4], fade: 0.0 });
        let Some(img) = render(&t, &green, &[], &cam, 1.0, &line, 1200, 800, [0, 0, 0, 255], &[]) else { return };
        let (x, y) = terrain::project(&t, &cam, 1.0, [1000.0, 1500.0, 0.0], 1200, 800).unwrap();
        let px = |dy: f32| -> [u8; 4] { let i = (((y + dy) as usize) * 1200 + x as usize) * 4; img[i..i + 4].try_into().unwrap() };
        // 40 % of the red over the green, no stronger where two segments meet.
        let faint = [(40.0 + 215.0 * 0.4) as u8, (160.0 * 0.6) as u8, (40.0 * 0.6) as u8];
        assert!(px(0.0)[..3].iter().zip(faint).all(|(a, b)| a.abs_diff(b) <= 2), "{:?}", px(0.0));
        assert_eq!(px(40.0), [40, 160, 40, 255]);
        assert!((-12..=12).all(|d| px(d as f32)[..3] != [0, 0, 0]), "no edging over the ridge");
    }

    #[test]
    #[ignore]
    fn timing_1920x1170_500x500() {
        let t = grid(500, |i, j| 800.0 + 300.0 * ((i as f32 / 20.0).sin() + (j as f32 / 15.0).cos()));
        let texture = Arc::new(gradient(1500, 0.12, 60));
        let insets: Vec<Arc<Inset>> = (0..4).map(|k| Arc::new(Inset { texture: gradient(1536, 0.0, 200), uv: [0.4 - 0.08 * k as f32, 0.4 - 0.08 * k as f32, 0.5 + 0.08 * k as f32, 0.5 + 0.08 * k as f32] })).collect();
        let cam = Camera { dist: 3000.0, pitch: 0.5, ..Camera::fit(&t) };
        let solids: Vec<Face> = (0..3_000).flat_map(|k| {
            let (x, y) = (cam.target[0] as f32 - 1200.0 + (k % 100) as f32 * 24.0, cam.target[1] as f32 - 1200.0 + (k / 100) as f32 * 24.0);
            let g = t.height_at(x as f64, y as f64) as f32;
            extrude(&[[x, y], [x + 20.0, y], [x + 20.0, y + 20.0], [x, y + 20.0]], g, g + 15.0, [150; 3], [200; 3])
        }).collect();
        let line: Vec<_> = (0..2000).map(|k| ([cam.target[0] - 2000.0 + k as f64 * 2.0, cam.target[1] + (k as f64 / 50.0).sin() * 800.0, 900.0], [255, 40, 40])).collect();
        let Some(_) = render(&t, &texture, &solids, &cam, 1.5, &line, 1920, 1170, [255; 4], &insets) else { return };
        let start = std::time::Instant::now();
        let runs = 20;
        for k in 0..runs {
            let cam = Camera { yaw: k as f64 * 0.01, ..cam };
            std::hint::black_box(render(&t, &texture, &solids, &cam, 1.5, &line, 1920, 1170, [255; 4], &insets));
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        println!("graphics card, 1920x1170, 500x500 vertices, {} faces, 4 insets: {ms:.1} ms", solids.len());
        assert!(ms < 16.0, "{ms} ms");
    }
}
