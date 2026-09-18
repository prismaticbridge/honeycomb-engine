use crate::{GPUTransform, vertex::{InterpolatedPose, Transform}};
use glam::{Affine2, Vec2};
use image;
use std::sync::Arc;

use crate::{GpuContext, buffer::GpuBuffer, vertex::TextureVertex};

pub struct Mesh {
    pub vertex_offset: u32,
    pub index_offset: u32,
    pub num_indices: u32,
    pub transformations: Vec<GPUTransform>, //each instance gets one transformation
    pub interpolated_poses: Vec<InterpolatedPose>,
    pub interpolated_transforms: Vec<GPUTransform>, //basically a temporary buffer
    pub direct_transform_buffer: GpuBuffer,
    pub interpolated_transform_buffer: GpuBuffer,
}

pub struct ColoredObject {
    pub start_index: u32,
    pub num_indices: u32,
}

pub struct Material {
    gpu: Arc<GpuContext>,
    pub bind_group: wgpu::BindGroup,
    //not needed now, but maybe later to support loading/unloading materials
    pub _texture: wgpu::Texture,
    pub _view: wgpu::TextureView,

    pub meshes: Vec<Mesh>,
    vertex_buffer: GpuBuffer,
    index_buffer: GpuBuffer,
}

impl Material {
    pub(crate) fn new(
        image_path: &str,
        gpu: &Arc<GpuContext>,
        bind_group_layout: &wgpu::BindGroupLayout,
        diffuse_sampler: &wgpu::Sampler,
    ) -> Result<Material, image::ImageError> {
        let rgb_image = image::ImageReader::open(image_path)?.decode()?.to_rgba8();
        let dimensions = rgb_image.dimensions();
        let texture_size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };
        let diffuse_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(image_path),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfoBase {
                texture: &diffuse_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgb_image,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(4 * dimensions.1),
            },
            texture_size,
        );

        let diffuse_texture_view =
            diffuse_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let diffuse_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(diffuse_sampler),
                },
            ],
        });

        Ok(Self {
            gpu: gpu.clone(),
            bind_group: diffuse_bind_group,
            _texture: diffuse_texture,
            _view: diffuse_texture_view,

            meshes: Vec::new(),
            vertex_buffer: GpuBuffer::new(gpu.clone(), wgpu::BufferUsages::VERTEX),
            index_buffer: GpuBuffer::new(gpu.clone(), wgpu::BufferUsages::INDEX),
        })
    }

    pub fn create_mesh(&mut self, mesh: &[TextureVertex], indices: &[u16]) -> usize {
        let new_mesh = Mesh {
            vertex_offset: self.index_buffer.bytes_used as u32 / size_of::<TextureVertex>() as u32,
            index_offset: self.vertex_buffer.bytes_used as u32 / size_of::<u16>() as u32,
            num_indices: indices.len() as u32,
            transformations: Vec::new(),
            interpolated_poses: Vec::new(),
            interpolated_transforms: Vec::new(),
            direct_transform_buffer: GpuBuffer::new(self.gpu.clone(), wgpu::BufferUsages::VERTEX),
            interpolated_transform_buffer: GpuBuffer::new(
                self.gpu.clone(),
                wgpu::BufferUsages::VERTEX,
            ),
        };
        self.vertex_buffer.append(bytemuck::cast_slice(mesh));
        self.index_buffer.append(indices);
        self.meshes.push(new_mesh);
        self.meshes.len() - 1
    }

    pub fn add_instance(&mut self, transform: &glam::Affine2, mesh: usize) {
        let mesh = &mut self.meshes[mesh];
        let size = mesh.transformations.len();
        mesh.transformations.push(GPUTransform::from(transform));
        mesh.direct_transform_buffer
            .append(bytemuck::cast_slice(&mesh.transformations[size..size + 1]));
    }

    pub fn add_interpolated_instance(&mut self, transform: &Transform, mesh: usize) {
        let mesh = &mut self.meshes[mesh];
        let interpolated_pose = InterpolatedPose::new(transform.clone());
        mesh.interpolated_poses.push(interpolated_pose);
        mesh.interpolated_transforms.push(GPUTransform::from(&Affine2::IDENTITY));
        let len = mesh.interpolated_transforms.len();
        mesh.interpolated_transform_buffer.append(bytemuck::cast_slice(&mesh.interpolated_transforms[len-1..len]));
    }

    pub fn move_object_absolute(&mut self, mesh: usize, object: usize, position: Vec2) {
        let mesh = &mut self.meshes[mesh];
        mesh.transformations[object].move_absolute(position);
        mesh.direct_transform_buffer.update_aligned(
            (object * size_of::<GPUTransform>()) as u32,
            bytemuck::cast_slice(&[mesh.transformations[object]]),
        );
    }

    //Note there is no move function for interpolated objects
    //It would literally do nothing but wrap the function in InterpolatedPose

    pub fn update_interpolations(&mut self, frame_timestamp: u64) {
        for mesh in &mut self.meshes {
            for i in 0..mesh.interpolated_poses.len() {
                let transform = mesh.interpolated_poses[i].interpolate(frame_timestamp);
                mesh.interpolated_transforms[i] = transform;
            }
            mesh.interpolated_transform_buffer.update_aligned(0, bytemuck::cast_slice(&mesh.interpolated_transforms));
        }
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass) {
        render_pass.set_bind_group(1, Some(&self.bind_group), &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.buffer.slice(..));
        render_pass.set_index_buffer(
            self.index_buffer.buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        for mesh in &self.meshes {
            render_pass.set_vertex_buffer(1, mesh.direct_transform_buffer.buffer.slice(..));
            render_pass.draw_indexed(
                mesh.index_offset..(mesh.index_offset + mesh.num_indices),
                mesh.vertex_offset as i32,
                0..mesh.transformations.len() as u32,
            );
            render_pass.set_vertex_buffer(1, mesh.interpolated_transform_buffer.buffer.slice(..));
            render_pass.draw_indexed(
                mesh.index_offset..(mesh.index_offset + mesh.num_indices),
                mesh.vertex_offset as i32,
                0..mesh.interpolated_transforms.len() as u32,
            );
        }
    }
}
