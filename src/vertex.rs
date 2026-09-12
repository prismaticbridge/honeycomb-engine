use bytemuck::{Pod, Zeroable};
use glam::{Affine2, Mat2, Vec2};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GPUTransform {
    col0: [f32; 2],
    col1: [f32; 2],
    pub translation: [f32; 2],
}

impl GPUTransform {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: size_of::<Vec2>() as u64,
                    shader_location: 3,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 2 * size_of::<Vec2>() as u64,
                    shader_location: 4,
                },
            ],
        }
    }

    pub fn move_absolute(&mut self, position: Vec2) {
        self.translation = position.to_array();
    }

    pub fn move_relative(&mut self, offset: Vec2) {
        self.translation[0] += offset.x;
        self.translation[1] += offset.y;
    }

    pub fn reset_transform(&mut self) {
        self.col0 = [1.0, 0.0];
        self.col1 = [0.0, 1.0];
    }

    pub fn apply_transform(&mut self, transform: &Mat2) {
        let t_row0 = transform.row(0);
        let t_row1 = transform.row(1);
        self.col0[0] = self.col0[0] * t_row0[0] + self.col0[1] * t_row0[1];
        self.col0[1] = self.col0[0] * t_row1[0] + self.col0[1] * t_row1[1];
        self.col1[0] = self.col1[0] * t_row0[0] + self.col1[1] * t_row0[1];
        self.col1[1] = self.col1[0] * t_row1[0] + self.col1[1] * t_row1[1];
    }
}

impl From<&glam::Affine2> for GPUTransform {
    fn from(src: &glam::Affine2) -> Self {
        let mat = src.matrix2;
        let t = src.translation;
        Self {
            col0: mat.col(0).into(),
            col1: mat.col(1).into(),
            translation: t.into(),
        }
    }
}

pub struct InterpolatedPose {
    target: Transform,
    current: Transform,
    start_time: u64,
    duration: u64,
}

impl InterpolatedPose {
    pub fn new(transform: Transform) -> Self {
        Self {
            target: transform.clone(),
            current: transform,
            start_time: 0,
            duration: 0,
        }
    }
    pub fn update_target(
        &mut self,
        transform: &Transform,
        current_time: u64,
        duration: u64,
    ) {
        self.target = transform.clone();
        self.start_time = current_time;
        self.duration = duration;
    }

    pub fn move_target_absolute(&mut self, new_position: Vec2, current_time: u64, duration: u64) {
        self.current = self.target.clone();
        self.target.position = new_position;
        self.start_time = current_time;
        self.duration = duration;
    }

    pub fn interpolate(&self, current_time: u64) -> GPUTransform {
        let elapsed = current_time.saturating_sub(self.start_time);
        let interpolate_point: f32 = elapsed as f32 / self.duration as f32;
        let transform = if self.duration == 0 || interpolate_point > 1.0 {
            &self.target
        } else {
            &Transform {
                position: self.target.position * interpolate_point
                    + self.current.position * (1.0 - interpolate_point),
                rotation: self.target.rotation * interpolate_point
                    + self.current.rotation * (1.0 - interpolate_point),
                scale: self.target.scale * interpolate_point
                    + self.current.scale * (1.0 - interpolate_point),
                shear: self.target.shear * interpolate_point
                    + self.current.shear * (1.0 - interpolate_point),
            }
        };
        let linear_transform: Mat2 = Mat2::from_scale_angle(transform.scale, transform.rotation);
        let shear_matrix =
            Mat2::from_cols(Vec2 { x: 1.0, y: transform.shear.y }, Vec2 { x: transform.shear.x, y: 1.0 });
        let affine_transform = Affine2 {
            matrix2: linear_transform * shear_matrix,
            translation: transform.position,
        };
        return GPUTransform::from(&affine_transform);
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 3],
}

impl Vertex {
    pub fn from_vector(position: &Vec2, color: [f32; 3]) -> Self {
        Self {
            position: position.to_array(),
            color,
        }
    }
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ], //attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct TextureVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}
impl TextureVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TextureVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[derive(Clone)]
pub struct Transform {
    pub position: Vec2,
    pub rotation: f32,
    pub shear: Vec2,
    pub scale: Vec2,
}

impl Transform {
    pub fn new() -> Self {
        Self { position: Vec2::ZERO, rotation: 0.0, shear: Vec2::ZERO, scale: Vec2::ONE }
    }
}
