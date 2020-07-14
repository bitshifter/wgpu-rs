use std::{iter, mem, ops::Range, rc::Rc};

#[path = "../framework.rs"]
mod framework;

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy)]

struct Vertex {
    _pos: [i8; 4],
    _normal: [i8; 4],
}

unsafe impl Pod for Vertex {}
unsafe impl Zeroable for Vertex {}

fn vertex(pos: [i8; 3], nor: [i8; 3]) -> Vertex {
    Vertex {
        _pos: [pos[0], pos[1], pos[2], 1],
        _normal: [nor[0], nor[1], nor[2], 0],
    }
}

fn create_cube() -> (Vec<Vertex>, Vec<u16>) {
    let vertex_data = [
        // top (0, 0, 1)
        vertex([-1, -1, 1], [0, 0, 1]),
        vertex([1, -1, 1], [0, 0, 1]),
        vertex([1, 1, 1], [0, 0, 1]),
        vertex([-1, 1, 1], [0, 0, 1]),
        // bottom (0, 0, -1)
        vertex([-1, 1, -1], [0, 0, -1]),
        vertex([1, 1, -1], [0, 0, -1]),
        vertex([1, -1, -1], [0, 0, -1]),
        vertex([-1, -1, -1], [0, 0, -1]),
        // right (1, 0, 0)
        vertex([1, -1, -1], [1, 0, 0]),
        vertex([1, 1, -1], [1, 0, 0]),
        vertex([1, 1, 1], [1, 0, 0]),
        vertex([1, -1, 1], [1, 0, 0]),
        // left (-1, 0, 0)
        vertex([-1, -1, 1], [-1, 0, 0]),
        vertex([-1, 1, 1], [-1, 0, 0]),
        vertex([-1, 1, -1], [-1, 0, 0]),
        vertex([-1, -1, -1], [-1, 0, 0]),
        // front (0, 1, 0)
        vertex([1, 1, -1], [0, 1, 0]),
        vertex([-1, 1, -1], [0, 1, 0]),
        vertex([-1, 1, 1], [0, 1, 0]),
        vertex([1, 1, 1], [0, 1, 0]),
        // back (0, -1, 0)
        vertex([1, -1, 1], [0, -1, 0]),
        vertex([-1, -1, 1], [0, -1, 0]),
        vertex([-1, -1, -1], [0, -1, 0]),
        vertex([1, -1, -1], [0, -1, 0]),
    ];

    let index_data: &[u16] = &[
        0, 1, 2, 2, 3, 0, // top
        4, 5, 6, 6, 7, 4, // bottom
        8, 9, 10, 10, 11, 8, // right
        12, 13, 14, 14, 15, 12, // left
        16, 17, 18, 18, 19, 16, // front
        20, 21, 22, 22, 23, 20, // back
    ];

    (vertex_data.to_vec(), index_data.to_vec())
}

fn create_plane(size: i8) -> (Vec<Vertex>, Vec<u16>) {
    let vertex_data = [
        vertex([size, -size, 0], [0, 0, 1]),
        vertex([size, size, 0], [0, 0, 1]),
        vertex([-size, -size, 0], [0, 0, 1]),
        vertex([-size, size, 0], [0, 0, 1]),
    ];

    let index_data: &[u16] = &[0, 1, 2, 2, 1, 3];

    (vertex_data.to_vec(), index_data.to_vec())
}

struct Entity {
    mx_world: glam::Mat4,
    rotation_speed: f32,
    color: wgpu::Color,
    vertex_buf: Rc<wgpu::Buffer>,
    index_buf: Rc<wgpu::Buffer>,
    index_count: usize,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
}

struct Light {
    pos: glam::Vec3,
    color: wgpu::Color,
    fov: f32,
    depth: Range<f32>,
    target_view: wgpu::TextureView,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LightRaw {
    proj: [[f32; 4]; 4],
    pos: [f32; 4],
    color: [f32; 4],
}

unsafe impl Pod for LightRaw {}
unsafe impl Zeroable for LightRaw {}

impl Light {
    fn to_raw(&self) -> LightRaw {
        let mx_view = glam::Mat4::look_at_rh(self.pos, glam::Vec3::zero(), glam::Vec3::unit_z());
        let mx_view_proj =
            glam::Mat4::perspective_rh(self.fov.to_radians(), 1.0, self.depth.start, self.depth.end) * mx_view;
        LightRaw {
            proj: mx_view_proj.to_cols_array_2d(),
            pos: [self.pos.x(), self.pos.y(), self.pos.z(), 1.0],
            color: [
                self.color.r as f32,
                self.color.g as f32,
                self.color.b as f32,
                1.0,
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ForwardUniforms {
    proj: [[f32; 4]; 4],
    num_lights: [u32; 4],
}

unsafe impl Pod for ForwardUniforms {}
unsafe impl Zeroable for ForwardUniforms {}

#[repr(C)]
#[derive(Clone, Copy)]
struct EntityUniforms {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

unsafe impl Pod for EntityUniforms {}
unsafe impl Zeroable for EntityUniforms {}

#[repr(C)]
struct ShadowUniforms {
    proj: [[f32; 4]; 4],
}

struct Pass {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
}

struct Example {
    entities: Vec<Entity>,
    lights: Vec<Light>,
    lights_are_dirty: bool,
    shadow_pass: Pass,
    forward_pass: Pass,
    forward_depth: wgpu::TextureView,
    light_uniform_buf: wgpu::Buffer,
}

impl Example {
    const MAX_LIGHTS: usize = 10;
    const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
    const SHADOW_SIZE: wgpu::Extent3d = wgpu::Extent3d {
        width: 512,
        height: 512,
        depth: Self::MAX_LIGHTS as u32,
    };
    const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    fn generate_matrix(aspect_ratio: f32) -> glam::Mat4 {
        let mx_projection = glam::Mat4::perspective_rh(45f32.to_radians(), aspect_ratio, 1.0, 20.0);
        let mx_view = glam::Mat4::look_at_rh(
            glam::Vec3::new(3.0, -10.0, 6.0),
            glam::Vec3::zero(),
            glam::Vec3::unit_z(),
        );
        mx_projection * mx_view
    }
}

impl framework::Example for Example {
    fn init(
        sc_desc: &wgpu::SwapChainDescriptor,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
    ) -> Self {
        // Create the vertex and index buffers
        let vertex_size = mem::size_of::<Vertex>();
        let (cube_vertex_data, cube_index_data) = create_cube();
        let cube_vertex_buf = Rc::new(device.create_buffer_with_data(
            bytemuck::cast_slice(&cube_vertex_data),
            wgpu::BufferUsage::VERTEX,
        ));

        let cube_index_buf = Rc::new(device.create_buffer_with_data(
            bytemuck::cast_slice(&cube_index_data),
            wgpu::BufferUsage::INDEX,
        ));

        let (plane_vertex_data, plane_index_data) = create_plane(7);
        let plane_vertex_buf = device.create_buffer_with_data(
            bytemuck::cast_slice(&plane_vertex_data),
            wgpu::BufferUsage::VERTEX,
        );

        let plane_index_buf = device.create_buffer_with_data(
            bytemuck::cast_slice(&plane_index_data),
            wgpu::BufferUsage::INDEX,
        );

        let entity_uniform_size = mem::size_of::<EntityUniforms>() as wgpu::BufferAddress;
        let plane_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: entity_uniform_size,
            usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });

        let local_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry::new(
                    0,
                    wgpu::ShaderStage::VERTEX | wgpu::ShaderStage::FRAGMENT,
                    wgpu::BindingType::UniformBuffer {
                        dynamic: false,
                        min_binding_size: wgpu::BufferSize::new(
                            mem::size_of::<EntityUniforms>() as _
                        ),
                    },
                )],
                label: None,
            });

        let mut entities = vec![{
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &local_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(plane_uniform_buf.slice(..)),
                }],
                label: None,
            });
            Entity {
                mx_world: glam::Mat4::identity(),
                rotation_speed: 0.0,
                color: wgpu::Color::WHITE,
                vertex_buf: Rc::new(plane_vertex_buf),
                index_buf: Rc::new(plane_index_buf),
                index_count: plane_index_data.len(),
                bind_group,
                uniform_buf: plane_uniform_buf,
            }
        }];

        struct CubeDesc {
            offset: glam::Vec3,
            angle: f32,
            scale: f32,
            rotation: f32,
        }
        let cube_descs = [
            CubeDesc {
                offset: glam::vec3(-2.0, -2.0, 2.0),
                angle: 10.0,
                scale: 0.7,
                rotation: 0.1,
            },
            CubeDesc {
                offset: glam::vec3(2.0, -2.0, 2.0),
                angle: 50.0,
                scale: 1.3,
                rotation: 0.2,
            },
            CubeDesc {
                offset: glam::vec3(-2.0, 2.0, 2.0),
                angle: 140.0,
                scale: 1.1,
                rotation: 0.3,
            },
            CubeDesc {
                offset: glam::vec3(2.0, 2.0, 2.0),
                angle: 210.0,
                scale: 0.9,
                rotation: 0.4,
            },
        ];

        for cube in &cube_descs {
            let disp = cube.offset;
            let rot = glam::Quat::from_axis_angle(cube.offset.normalize(), cube.angle.to_radians());
            let scale = glam::Vec3::splat(cube.scale);
            let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: entity_uniform_size,
                usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            });
            entities.push(Entity {
                mx_world: glam::Mat4::from_scale_rotation_translation(scale, rot, disp),
                rotation_speed: cube.rotation,
                color: wgpu::Color::GREEN,
                vertex_buf: Rc::clone(&cube_vertex_buf),
                index_buf: Rc::clone(&cube_index_buf),
                index_count: cube_index_data.len(),
                bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &local_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(uniform_buf.slice(..)),
                    }],
                    label: None,
                }),
                uniform_buf,
            });
        }

        // Create other resources
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: Self::SHADOW_SIZE,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::SHADOW_FORMAT,
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT | wgpu::TextureUsage::SAMPLED,
            label: None,
        });
        let shadow_view = shadow_texture.create_default_view();

        let mut shadow_target_views = (0..2)
            .map(|i| {
                Some(shadow_texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("shadow"),
                    format: Self::SHADOW_FORMAT,
                    dimension: wgpu::TextureViewDimension::D2,
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: i as u32,
                    array_layer_count: 1,
                }))
            })
            .collect::<Vec<_>>();
        let lights = vec![
            Light {
                pos: glam::Vec3::new(7.0, -5.0, 10.0),
                color: wgpu::Color {
                    r: 0.5,
                    g: 1.0,
                    b: 0.5,
                    a: 1.0,
                },
                fov: 60.0,
                depth: 1.0..20.0,
                target_view: shadow_target_views[0].take().unwrap(),
            },
            Light {
                pos: glam::Vec3::new(-5.0, 7.0, 10.0),
                color: wgpu::Color {
                    r: 1.0,
                    g: 0.5,
                    b: 0.5,
                    a: 1.0,
                },
                fov: 45.0,
                depth: 1.0..20.0,
                target_view: shadow_target_views[1].take().unwrap(),
            },
        ];
        let light_uniform_size =
            (Self::MAX_LIGHTS * mem::size_of::<LightRaw>()) as wgpu::BufferAddress;
        let light_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: light_uniform_size,
            usage: wgpu::BufferUsage::UNIFORM
                | wgpu::BufferUsage::COPY_SRC
                | wgpu::BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });

        let vb_desc = wgpu::VertexBufferDescriptor {
            stride: vertex_size as wgpu::BufferAddress,
            step_mode: wgpu::InputStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Char4, 1 => Char4],
        };

        let shadow_pass = {
            let uniform_size = mem::size_of::<ShadowUniforms>() as wgpu::BufferAddress;
            // Create pipeline layout
            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry::new(
                        0, // global
                        wgpu::ShaderStage::VERTEX,
                        wgpu::BindingType::UniformBuffer {
                            dynamic: false,
                            min_binding_size: wgpu::BufferSize::new(uniform_size),
                        },
                    )],
                    label: None,
                });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                bind_group_layouts: &[&bind_group_layout, &local_bind_group_layout],
                push_constant_ranges: &[],
            });

            let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: uniform_size,
                usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
                mapped_at_creation: false,
            });

            // Create bind group
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(uniform_buf.slice(..)),
                }],
                label: None,
            });

            // Create the render pipeline
            let vs_module = device.create_shader_module(wgpu::include_spirv!("bake.vert.spv"));
            let fs_module = device.create_shader_module(wgpu::include_spirv!("bake.frag.spv"));

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                layout: &pipeline_layout,
                vertex_stage: wgpu::ProgrammableStageDescriptor {
                    module: &vs_module,
                    entry_point: "main",
                },
                fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                    module: &fs_module,
                    entry_point: "main",
                }),
                rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: wgpu::CullMode::Back,
                    depth_bias: 2, // corresponds to bilinear filtering
                    depth_bias_slope_scale: 2.0,
                    depth_bias_clamp: 0.0,
                }),
                primitive_topology: wgpu::PrimitiveTopology::TriangleList,
                color_states: &[],
                depth_stencil_state: Some(wgpu::DepthStencilStateDescriptor {
                    format: Self::SHADOW_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil_front: wgpu::StencilStateFaceDescriptor::IGNORE,
                    stencil_back: wgpu::StencilStateFaceDescriptor::IGNORE,
                    stencil_read_mask: 0,
                    stencil_write_mask: 0,
                }),
                vertex_state: wgpu::VertexStateDescriptor {
                    index_format: wgpu::IndexFormat::Uint16,
                    vertex_buffers: &[vb_desc.clone()],
                },
                sample_count: 1,
                sample_mask: !0,
                alpha_to_coverage_enabled: false,
            });

            Pass {
                pipeline,
                bind_group,
                uniform_buf,
            }
        };

        let forward_pass = {
            // Create pipeline layout
            let bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[
                        wgpu::BindGroupLayoutEntry::new(
                            0, // global
                            wgpu::ShaderStage::VERTEX | wgpu::ShaderStage::FRAGMENT,
                            wgpu::BindingType::UniformBuffer {
                                dynamic: false,
                                min_binding_size: wgpu::BufferSize::new(mem::size_of::<
                                    ForwardUniforms,
                                >(
                                )
                                    as _),
                            },
                        ),
                        wgpu::BindGroupLayoutEntry::new(
                            1, // lights
                            wgpu::ShaderStage::VERTEX | wgpu::ShaderStage::FRAGMENT,
                            wgpu::BindingType::UniformBuffer {
                                dynamic: false,
                                min_binding_size: wgpu::BufferSize::new(light_uniform_size),
                            },
                        ),
                        wgpu::BindGroupLayoutEntry::new(
                            2,
                            wgpu::ShaderStage::FRAGMENT,
                            wgpu::BindingType::SampledTexture {
                                multisampled: false,
                                component_type: wgpu::TextureComponentType::Float,
                                dimension: wgpu::TextureViewDimension::D2Array,
                            },
                        ),
                        wgpu::BindGroupLayoutEntry::new(
                            3,
                            wgpu::ShaderStage::FRAGMENT,
                            wgpu::BindingType::Sampler { comparison: true },
                        ),
                    ],
                    label: None,
                });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                bind_group_layouts: &[&bind_group_layout, &local_bind_group_layout],
                push_constant_ranges: &[],
            });

            let mx_total = Self::generate_matrix(sc_desc.width as f32 / sc_desc.height as f32);
            let forward_uniforms = ForwardUniforms {
                proj: mx_total.to_cols_array_2d(),
                num_lights: [lights.len() as u32, 0, 0, 0],
            };
            let uniform_buf = device.create_buffer_with_data(
                bytemuck::bytes_of(&forward_uniforms),
                wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            );

            // Create bind group
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(uniform_buf.slice(..)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(light_uniform_buf.slice(..)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                    },
                ],
                label: None,
            });

            // Create the render pipeline
            let vs_module = device.create_shader_module(wgpu::include_spirv!("forward.vert.spv"));
            let fs_module = device.create_shader_module(wgpu::include_spirv!("forward.frag.spv"));

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                layout: &pipeline_layout,
                vertex_stage: wgpu::ProgrammableStageDescriptor {
                    module: &vs_module,
                    entry_point: "main",
                },
                fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                    module: &fs_module,
                    entry_point: "main",
                }),
                rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: wgpu::CullMode::Back,
                    depth_bias: 0,
                    depth_bias_slope_scale: 0.0,
                    depth_bias_clamp: 0.0,
                }),
                primitive_topology: wgpu::PrimitiveTopology::TriangleList,
                color_states: &[wgpu::ColorStateDescriptor {
                    format: sc_desc.format,
                    color_blend: wgpu::BlendDescriptor::REPLACE,
                    alpha_blend: wgpu::BlendDescriptor::REPLACE,
                    write_mask: wgpu::ColorWrite::ALL,
                }],
                depth_stencil_state: Some(wgpu::DepthStencilStateDescriptor {
                    format: Self::DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil_front: wgpu::StencilStateFaceDescriptor::IGNORE,
                    stencil_back: wgpu::StencilStateFaceDescriptor::IGNORE,
                    stencil_read_mask: 0,
                    stencil_write_mask: 0,
                }),
                vertex_state: wgpu::VertexStateDescriptor {
                    index_format: wgpu::IndexFormat::Uint16,
                    vertex_buffers: &[vb_desc],
                },
                sample_count: 1,
                sample_mask: !0,
                alpha_to_coverage_enabled: false,
            });

            Pass {
                pipeline,
                bind_group,
                uniform_buf,
            }
        };

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: sc_desc.width,
                height: sc_desc.height,
                depth: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::DEPTH_FORMAT,
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            label: None,
        });

        Example {
            entities,
            lights,
            lights_are_dirty: true,
            shadow_pass,
            forward_pass,
            forward_depth: depth_texture.create_default_view(),
            light_uniform_buf,
        }
    }

    fn update(&mut self, _event: winit::event::WindowEvent) {
        //empty
    }

    fn resize(
        &mut self,
        sc_desc: &wgpu::SwapChainDescriptor,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        // update view-projection matrix
        let mx_total = Self::generate_matrix(sc_desc.width as f32 / sc_desc.height as f32);
        let mx_ref: &[f32; 16] = mx_total.as_ref();
        queue.write_buffer(
            &self.forward_pass.uniform_buf,
            0,
            bytemuck::cast_slice(mx_ref),
        );

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: sc_desc.width,
                height: sc_desc.height,
                depth: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::DEPTH_FORMAT,
            usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
            label: None,
        });
        self.forward_depth = depth_texture.create_default_view();
    }

    fn render(
        &mut self,
        frame: &wgpu::SwapChainTexture,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _spawner: &impl futures::task::LocalSpawn,
    ) {
        // update uniforms
        for entity in self.entities.iter_mut() {
            if entity.rotation_speed != 0.0 {
                let rotation = glam::Mat4::from_rotation_x(entity.rotation_speed.to_radians());
                entity.mx_world = entity.mx_world * rotation;
            }
            let data = EntityUniforms {
                model: entity.mx_world.to_cols_array_2d(),
                color: [
                    entity.color.r as f32,
                    entity.color.g as f32,
                    entity.color.b as f32,
                    entity.color.a as f32,
                ],
            };
            queue.write_buffer(&entity.uniform_buf, 0, bytemuck::bytes_of(&data));
        }

        if self.lights_are_dirty {
            self.lights_are_dirty = false;
            for (i, light) in self.lights.iter().enumerate() {
                queue.write_buffer(
                    &self.light_uniform_buf,
                    (i * mem::size_of::<LightRaw>()) as wgpu::BufferAddress,
                    bytemuck::bytes_of(&light.to_raw()),
                );
            }
        }

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        for (i, light) in self.lights.iter().enumerate() {
            // The light uniform buffer already has the projection,
            // let's just copy it over to the shadow uniform buffer.
            encoder.copy_buffer_to_buffer(
                &self.light_uniform_buf,
                (i * mem::size_of::<LightRaw>()) as wgpu::BufferAddress,
                &self.shadow_pass.uniform_buf,
                0,
                64,
            );

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachmentDescriptor {
                    attachment: &light.target_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: true,
                    }),
                    stencil_ops: None,
                }),
            });
            pass.set_pipeline(&self.shadow_pass.pipeline);
            pass.set_bind_group(0, &self.shadow_pass.bind_group, &[]);

            for entity in &self.entities {
                pass.set_bind_group(1, &entity.bind_group, &[]);
                pass.set_index_buffer(entity.index_buf.slice(..));
                pass.set_vertex_buffer(0, entity.vertex_buf.slice(..));
                pass.draw_indexed(0..entity.index_count as u32, 0, 0..1);
            }
        }

        // forward pass
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: true,
                    },
                }],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachmentDescriptor {
                    attachment: &self.forward_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: false,
                    }),
                    stencil_ops: None,
                }),
            });
            pass.set_pipeline(&self.forward_pass.pipeline);
            pass.set_bind_group(0, &self.forward_pass.bind_group, &[]);

            for entity in &self.entities {
                pass.set_bind_group(1, &entity.bind_group, &[]);
                pass.set_index_buffer(entity.index_buf.slice(..));
                pass.set_vertex_buffer(0, entity.vertex_buf.slice(..));
                pass.draw_indexed(0..entity.index_count as u32, 0, 0..1);
            }
        }

        queue.submit(iter::once(encoder.finish()));
    }
}

fn main() {
    framework::run::<Example>("shadow");
}
