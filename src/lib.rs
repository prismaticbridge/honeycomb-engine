use crate::{
    material::ColoredObject,
    scene::Scene,
    utils::{SurfaceError, try_create_surface},
    vertex::{GPUTransform, TextureVertex, Vertex},
};
use glam::{Affine2, Mat2, Vec2};
use std::{num::NonZeroU64, sync::Arc};
use std::{path::PathBuf, sync::Mutex};
use wgpu::util::DeviceExt;
use winit::{
    event_loop::{ActiveEventLoop, EventLoop},
    window::Window,
};

mod buffer;
mod material;
mod scene;
pub mod utils;
pub mod vertex;

pub fn create_event_loop() -> EventLoop<()> {
    EventLoop::new().unwrap()
}

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

pub struct Renderer {
    ///Window isn't used in renderer, the application should hold a separate Arc clone
    pub window: Arc<Window>,
    pub is_surface_configured: Mutex<bool>, // so render doesn't require mutable reference and can be run asynchronously
    gpu: Arc<GpuContext>,
    config: wgpu::SurfaceConfiguration,

    pub window_width: u32,
    pub window_height: u32,

    basic_render_pipeline: wgpu::RenderPipeline,
    texture_render_pipeline: wgpu::RenderPipeline,

    texture_bind_group_layout: wgpu::BindGroupLayout,

    camera_transform: GPUTransform,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    pub scenes: Vec<Scene>,
    pub active_scene: Option<usize>,
    surface: wgpu::Surface<'static>,

    asset_root: PathBuf,
}

impl Renderer {
    pub async fn new(event_loop: &ActiveEventLoop) -> Self {
        //Window creation
        let window_attributes = Window::default_attributes();
        let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
        window.set_visible(true);
        window.request_redraw();

        //Instance
        let size = window.inner_size();
        let window_width = size.width;
        let window_height = size.height;

        let (instance, surface) = try_create_surface(window.clone());
        let eventual_adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        });

        let adapter = eventual_adapter.await.unwrap();

        //Device and queue creation
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        println!("Chosen format: {:?}", surface_format);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        //Basic shader initialization
        let basic_shader =
            device.create_shader_module(wgpu::include_wgsl!("../shaders/basic.wgsl"));
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Basic render pipeline layout"),
                bind_group_layouts: &[Some(&uniform_bind_group_layout)],
                immediate_size: 0,
            });

        //Shared states between render pipelines
        let primitive_state = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        };
        let multisample_state = wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let basic_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("basic pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &basic_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(Vertex::desc())],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &basic_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: primitive_state,
                depth_stencil: None,
                multisample: multisample_state,
                multiview_mask: None,
                cache: None,
            });

        let texture_shader =
            device.create_shader_module(wgpu::include_wgsl!("../shaders/texture.wgsl"));
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture bind group layout"),
                //Slot 0 is texture, slot 1 is sampler
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let camera_transform = GPUTransform::from(&glam::Affine2::IDENTITY);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniform buffer init descriptor"),
            contents: bytemuck::cast_slice(&[camera_transform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: NonZeroU64::new(size_of::<GPUTransform>() as u64),
                }),
            }],
        });

        let texture_render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("texture pipeline"),
                bind_group_layouts: &[
                    Some(&uniform_bind_group_layout),
                    Some(&texture_bind_group_layout),
                ],
                immediate_size: 0,
            });

        let texture_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("texture pipeline"),
                layout: Some(&texture_render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &texture_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(TextureVertex::desc()), Some(GPUTransform::desc())],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &texture_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: primitive_state,
                depth_stencil: None,
                multisample: multisample_state,
                multiview_mask: None,
                cache: None,
            });

        let gpu = Arc::new(GpuContext { device, queue });

        //really annoying path parsing stuff
        // let asset_root = {
        //     if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        //         //Running from cargo crate so use cargo path
        //         let mut path = PathBuf::from(manifest_dir);
        //         path.push("resources");
        //         path
        //     } else {
        //         //running from executable so get root manually
        //         let mut exe_path = std::env::current_exe().expect("Error: failed to get exe path");
        //         exe_path.pop();
        //         exe_path.push("resources");
        //         exe_path
        //     }
        // };
        //generated by gemini
        let asset_root = {
            // env! evaluates at compile time, so it always catches CARGO_MANIFEST_DIR
            // regardless of how or where the executable is launched.
            if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
                PathBuf::from(manifest_dir)
                    .parent()
                    .unwrap()
                    .join("resources")
            } else {
                // Fallback for standalone release distributions outside of cargo
                let mut exe_path = std::env::current_exe().expect("Error: failed to get exe path");
                exe_path.pop();
                exe_path.push("resources");
                exe_path
            }
        };

        Self {
            surface,
            gpu,
            config,
            is_surface_configured: Mutex::new(false),
            window_width,
            window_height,
            window,
            basic_render_pipeline,
            texture_bind_group_layout,
            texture_render_pipeline,
            camera_transform,
            uniform_buffer,
            uniform_bind_group,
            scenes: Vec::new(),
            active_scene: None,
            asset_root,
        }
    }

    pub fn create_scene(&mut self) -> usize {
        self.scenes.push(Scene::new(self.gpu.clone()));
        self.active_scene = Some(self.scenes.len() - 1);
        self.scenes.len() - 1
    }

    pub fn add_static_object(
        &mut self,
        scene: usize,
        vertices: &[Vertex],
        indices: &[u16],
    ) -> ColoredObject {
        let scene_obj = &mut self.scenes[scene];
        scene_obj.add_static_object(vertices, indices)
    }

    //use weird generic to allow passing by string or by std::path::Path
    pub fn add_scene_material<T: AsRef<std::path::Path>>(
        &mut self,
        scene: usize,
        texture_image_path: T,
    ) -> usize {
        let path = self.asset_root.join(texture_image_path);
        println!("{}", path.display());
        self.scenes[scene].add_material(&path.to_string_lossy(), &self.texture_bind_group_layout)
    }

    pub fn add_scene_object(
        &mut self,
        scene: usize,
        material: usize,
        vertices: &[TextureVertex],
        indices: &[u16],
    ) -> usize {
        self.scenes[scene].materials[material].create_mesh(vertices, indices)
    }

    pub fn add_object_instance(
        &mut self,
        scene: usize,
        material: usize,
        mesh: usize,
        transform: &Affine2,
    ) -> usize {
        let material_ref = &mut self.scenes[scene].materials[material];
        material_ref.add_instance(transform, mesh);
        // Object { scene: scene as u16, material: material as u16, mesh: mesh as u16, index: material_ref.meshes.len() as u16 - 1 }
        material_ref.meshes[mesh].transformations.len() - 1
    }

    pub fn render(&self) -> Result<(), utils::SurfaceError> {
        if let Ok(mut surface_configured) = self.is_surface_configured.lock()
            && !*surface_configured
        {
            self.surface.configure(&self.gpu.device, &self.config);
            *surface_configured = true;
        };

        // self.apply_camera_transform(Vec2 { x: 1.0, y: 1.0 }, 0.001);

        let output_maybe = self.surface.get_current_texture();
        let output = match output_maybe {
            wgpu::CurrentSurfaceTexture::Success(surface) => surface,
            wgpu::CurrentSurfaceTexture::Suboptimal(surface) => {
                if let Ok(mut surface_configured) = self.is_surface_configured.lock() {
                    *surface_configured = false;
                };
                surface
            }
            wgpu::CurrentSurfaceTexture::Timeout => return Err(SurfaceError::Timeout),
            wgpu::CurrentSurfaceTexture::Lost => return Err(SurfaceError::Lost),
            wgpu::CurrentSurfaceTexture::Occluded => return Err(SurfaceError::Occluded),
            wgpu::CurrentSurfaceTexture::Validation => return Err(SurfaceError::Validation),
            wgpu::CurrentSurfaceTexture::Outdated => return Err(SurfaceError::Outdated),
        };

        let scene = match self.active_scene {
            None => {
                println!("Warning: no scene selected, skipping rendering");
                return Ok(());
            }
            Some(scene_index) => &self.scenes[scene_index],
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.basic_render_pipeline);
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[]); // need to be set once per render pass
            scene.render_static(&mut render_pass);

            render_pass.set_pipeline(&self.texture_render_pipeline);
            scene.render_materials(&mut render_pass);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        self.gpu.queue.present(output);

        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
        if let Ok(mut surface_configured) = self.is_surface_configured.lock() {
            *surface_configured = true;
        };
    }

    pub fn move_camera(&mut self, offset: Vec2) {
        self.camera_transform.move_relative(-offset);
    }

    /// Sets the camera to an absolute world position
    pub fn move_camera_absolute(&mut self, position: Vec2) {
        self.camera_transform.move_absolute(-position);
    }

    pub fn set_transform(&mut self, scale: Vec2, angle: f32) {
        self.camera_transform.reset_transform();
        let transformation_matrix = Mat2::from_scale_angle(scale, angle);
        self.camera_transform
            .apply_transform(&transformation_matrix);
    }

    pub fn update_camera(&mut self) {
        self.gpu.queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_transform]),
        );
    }

    pub fn camera_position(&self) -> Vec2 {
        return -Vec2::from_array(self.camera_transform.translation); // camera position is secretly inverted
    }

    pub fn move_object(&mut self, material: usize, mesh: usize, object: usize, position: Vec2) {
        let material = &mut self.scenes[self.active_scene.unwrap()].materials[material];
        material.move_object_absolute(mesh, object, position);
    }

    //Maybe too much complexity? ignore for now
    // pub fn move_object(&mut)

    // pub fn get_object(&self, material: usize, mesh: usize, instance_index: usize) -> Object {
    //     Object {
    //         scene: self.active_scene.unwrap() as u16,
    //         material: material as u16,
    //         mesh: mesh as u16,
    //         index: instance_index as u16,
    //     }
    // }
}
