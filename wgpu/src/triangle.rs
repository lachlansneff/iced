//! Draw meshes of triangles.
use crate::{settings, Transformation};
use iced_graphics::layer;
use std::mem;
use zerocopy::AsBytes;

pub use iced_graphics::triangle::{Mesh2D, Vertex2D};

mod msaa;

const UNIFORM_BUFFER_SIZE: usize = 100;
const VERTEX_BUFFER_SIZE: usize = 10_000;
const INDEX_BUFFER_SIZE: usize = 10_000;

pub(crate) struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    blit: Option<msaa::Blit>,
    constants: wgpu::BindGroup,
    uniforms_buffer: Buffer<Uniforms>,
    vertex_buffer: Buffer<Vertex2D>,
    index_buffer: Buffer<u32>,
}

struct Buffer<T> {
    raw: wgpu::Buffer,
    size: usize,
    usage: wgpu::BufferUsage,
    _type: std::marker::PhantomData<T>,
}

impl<T> Buffer<T> {
    pub fn new(
        device: &wgpu::Device,
        size: usize,
        usage: wgpu::BufferUsage,
    ) -> Self {
        let raw = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (std::mem::size_of::<T>() * size) as u64,
            usage,
            mapped_at_creation: false,
        });

        Buffer {
            raw,
            size,
            usage,
            _type: std::marker::PhantomData,
        }
    }

    pub fn ensure_capacity(&mut self, device: &wgpu::Device, size: usize) {
        if self.size < size {
            self.raw = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (std::mem::size_of::<T>() * size) as u64,
                usage: self.usage,
                mapped_at_creation: false,
            });

            self.size = size;
        }
    }
}

impl Pipeline {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        antialiasing: Option<settings::Antialiasing>,
    ) -> Pipeline {
        let constant_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                bindings: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStage::VERTEX,
                    ty: wgpu::BindingType::UniformBuffer { dynamic: true },
                    ..Default::default()
                }],
            });

        let constants_buffer = Buffer::new(
            device,
            UNIFORM_BUFFER_SIZE,
            wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
        );

        let constant_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &constant_layout,
                bindings: &[wgpu::Binding {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(constants_buffer.raw.slice(..std::mem::size_of::<Uniforms>() as u64)),
                }],
            });

        let layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                bind_group_layouts: &[&constant_layout],
            });

        let vs = include_bytes!("shader/triangle.vert.spv");
        let vs_module = device.create_shader_module(
            &wgpu::read_spirv(std::io::Cursor::new(&vs[..]))
                .expect("Read triangle vertex shader as SPIR-V"),
        );

        let fs = include_bytes!("shader/triangle.frag.spv");
        let fs_module = device.create_shader_module(
            &wgpu::read_spirv(std::io::Cursor::new(&fs[..]))
                .expect("Read triangle fragment shader as SPIR-V"),
        );

        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                layout: &layout,
                vertex_stage: wgpu::ProgrammableStageDescriptor {
                    module: &vs_module,
                    entry_point: "main",
                },
                fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                    module: &fs_module,
                    entry_point: "main",
                }),
                rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: wgpu::CullMode::None,
                    depth_bias: 0,
                    depth_bias_slope_scale: 0.0,
                    depth_bias_clamp: 0.0,
                }),
                primitive_topology: wgpu::PrimitiveTopology::TriangleList,
                color_states: &[wgpu::ColorStateDescriptor {
                    format,
                    color_blend: wgpu::BlendDescriptor {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha_blend: wgpu::BlendDescriptor {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    write_mask: wgpu::ColorWrite::ALL,
                }],
                depth_stencil_state: None,
                vertex_state: wgpu::VertexStateDescriptor {
                    index_format: wgpu::IndexFormat::Uint32,
                    vertex_buffers: &[wgpu::VertexBufferDescriptor {
                        stride: mem::size_of::<Vertex2D>() as u64,
                        step_mode: wgpu::InputStepMode::Vertex,
                        attributes: &[
                            // Position
                            wgpu::VertexAttributeDescriptor {
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float2,
                                offset: 0,
                            },
                            // Color
                            wgpu::VertexAttributeDescriptor {
                                shader_location: 1,
                                format: wgpu::VertexFormat::Float4,
                                offset: 4 * 2,
                            },
                        ],
                    }],
                },
                sample_count: u32::from(
                    antialiasing.map(|a| a.sample_count()).unwrap_or(1),
                ),
                sample_mask: !0,
                alpha_to_coverage_enabled: false,
            });

        Pipeline {
            pipeline,
            blit: antialiasing.map(|a| msaa::Blit::new(device, format, a)),
            constants: constant_bind_group,
            uniforms_buffer: constants_buffer,
            vertex_buffer: Buffer::new(
                device,
                VERTEX_BUFFER_SIZE,
                wgpu::BufferUsage::VERTEX | wgpu::BufferUsage::COPY_DST,
            ),
            index_buffer: Buffer::new(
                device,
                INDEX_BUFFER_SIZE,
                wgpu::BufferUsage::INDEX | wgpu::BufferUsage::COPY_DST,
            ),
        }
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_width: u32,
        target_height: u32,
        transformation: Transformation,
        scale_factor: f32,
        meshes: &[layer::Mesh<'_>],
    ) {
        // This looks a bit crazy, but we are just counting how many vertices
        // and indices we will need to handle.
        // TODO: Improve readability
        let (total_vertices, total_indices) = meshes
            .iter()
            .map(|layer::Mesh { buffers, .. }| {
                (buffers.vertices.len(), buffers.indices.len())
            })
            .fold((0, 0), |(total_v, total_i), (v, i)| {
                (total_v + v, total_i + i)
            });

        // Then we ensure the current buffers are big enough, resizing if
        // necessary
        self.uniforms_buffer.ensure_capacity(device, meshes.len());
        self.vertex_buffer.ensure_capacity(device, total_vertices);
        self.index_buffer.ensure_capacity(device, total_indices);

        let mut uniforms: Vec<Uniforms> = Vec::with_capacity(meshes.len());
        let mut offsets: Vec<(
            wgpu::BufferAddress,
            wgpu::BufferAddress,
            usize,
        )> = Vec::with_capacity(meshes.len());
        let mut last_vertex = 0;
        let mut last_index = 0;

        // We upload everything upfront
        for mesh in meshes {
            let transform = (transformation
                * Transformation::translate(mesh.origin.x, mesh.origin.y))
            .into();

            let vertex_buffer = device.create_buffer_with_data(
                bytemuck::cast_slice(&mesh.buffers.vertices),
                wgpu::BufferUsage::COPY_SRC,
            );

            let index_buffer = device.create_buffer_with_data(
                mesh.buffers.indices.as_bytes(),
                wgpu::BufferUsage::COPY_SRC,
            );

            encoder.copy_buffer_to_buffer(
                &vertex_buffer,
                0,
                &self.vertex_buffer.raw,
                (std::mem::size_of::<Vertex2D>() * last_vertex) as u64,
                (std::mem::size_of::<Vertex2D>() * mesh.buffers.vertices.len())
                    as u64,
            );

            encoder.copy_buffer_to_buffer(
                &index_buffer,
                0,
                &self.index_buffer.raw,
                (std::mem::size_of::<u32>() * last_index) as u64,
                (std::mem::size_of::<u32>() * mesh.buffers.indices.len())
                    as u64,
            );

            uniforms.push(transform);
            offsets.push((
                last_vertex as u64,
                last_index as u64,
                mesh.buffers.indices.len(),
            ));

            last_vertex += mesh.buffers.vertices.len();
            last_index += mesh.buffers.indices.len();
        }

        let uniforms_buffer = device.create_buffer_with_data(
            uniforms.as_bytes(),
            wgpu::BufferUsage::COPY_SRC,
        );

        encoder.copy_buffer_to_buffer(
            &uniforms_buffer,
            0,
            &self.uniforms_buffer.raw,
            0,
            (std::mem::size_of::<Uniforms>() * uniforms.len()) as u64,
        );

        {
            let (attachment, resolve_target, load_op) =
                if let Some(blit) = &mut self.blit {
                    let (attachment, resolve_target) =
                        blit.targets(device, target_width, target_height);

                    (attachment, Some(resolve_target), wgpu::LoadOp::Clear)
                } else {
                    (target, None, wgpu::LoadOp::Load)
                };

            let mut render_pass =
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[
                        wgpu::RenderPassColorAttachmentDescriptor {
                            attachment,
                            resolve_target,
                            load_op,
                            store_op: wgpu::StoreOp::Store,
                            clear_color: wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 0.0,
                            },
                        },
                    ],
                    depth_stencil_attachment: None,
                });

            render_pass.set_pipeline(&self.pipeline);

            for (i, (vertex_offset, index_offset, indices)) in
                offsets.into_iter().enumerate()
            {
                let clip_bounds = (meshes[i].clip_bounds * scale_factor).snap();

                render_pass.set_scissor_rect(
                    clip_bounds.x,
                    clip_bounds.y,
                    clip_bounds.width,
                    clip_bounds.height,
                );

                render_pass.set_bind_group(
                    0,
                    &self.constants,
                    &[(std::mem::size_of::<Uniforms>() * i) as u32],
                );

                render_pass.set_index_buffer(
                    self.index_buffer.raw.slice(index_offset * std::mem::size_of::<u32>() as u64..),
                );

                render_pass.set_vertex_buffer(
                    0,
                    self.vertex_buffer.raw.slice(vertex_offset * std::mem::size_of::<Vertex2D>() as u64..),
                );

                render_pass.draw_indexed(0..indices as u32, 0, 0..1);
            }
        }

        if let Some(blit) = &mut self.blit {
            blit.draw(encoder, target);
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes)]
struct Uniforms {
    transform: [f32; 16],
    // We need to align this to 256 bytes to please `wgpu`...
    // TODO: Be smarter and stop wasting memory!
    _padding_a: [f32; 32],
    _padding_b: [f32; 16],
}

impl Default for Uniforms {
    fn default() -> Self {
        Self {
            transform: *Transformation::identity().as_ref(),
            _padding_a: [0.0; 32],
            _padding_b: [0.0; 16],
        }
    }
}

impl From<Transformation> for Uniforms {
    fn from(transformation: Transformation) -> Uniforms {
        Self {
            transform: transformation.into(),
            _padding_a: [0.0; 32],
            _padding_b: [0.0; 16],
        }
    }
}
