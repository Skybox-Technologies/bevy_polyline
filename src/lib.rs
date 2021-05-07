mod pipeline;

use std::sync::Arc;

use bevy::{
    core::{AsBytes, Bytes},
    ecs::{reflect::ReflectComponent, system::IntoSystem, world::WorldCell},
    math::{Mat4, Vec3},
    prelude::{
        Assets, Changed, ClearColor, Commands, Draw, Entity, GlobalTransform, Handle,
        HandleUntyped, Msaa, Query, QuerySet, RenderPipelines, Res, ResMut, Shader, Transform,
        With, Without, World,
    },
    reflect::{Reflect, TypeUuid},
    render::{
        camera::ActiveCameras,
        draw::{DrawContext, OutsideFrustum},
        pass::{LoadOp, PassDescriptor, TextureAttachment},
        pipeline::{
            BindGroupDescriptorId, IndexFormat, InputStepMode, PipelineCompiler,
            PipelineDescriptor, PipelineSpecialization, VertexAttribute, VertexFormat,
        },
        render_graph::{
            base::{self, MainPass},
            Node, ResourceSlotInfo,
        },
        renderer::{
            BindGroupId, BufferId, BufferInfo, BufferUsage, RenderResourceBindings,
            RenderResourceContext, RenderResourceType,
        },
        RenderStage,
    },
    utils::HashSet,
};
use bevy::{
    prelude::{Bundle, Plugin, Visible},
    render::pipeline::VertexBufferLayout,
};

pub const POLY_LINE_PIPELINE_HANDLE: HandleUntyped =
    HandleUntyped::weak_from_u64(PipelineDescriptor::TYPE_UUID, 0x6e339e9dad279849);

pub const INSTANCED_POLY_LINE_PIPELINE_HANDLE: HandleUntyped =
    HandleUntyped::weak_from_u64(PipelineDescriptor::TYPE_UUID, 0x6e339e9dad279498);

pub struct PolyLinePlugin;

impl Plugin for PolyLinePlugin {
    fn build(&self, app: &mut bevy::prelude::AppBuilder) {
        app.register_type::<PolyLine>()
            // .add_startup_system(setup_specialized_pipeline.system())
            .add_system_to_stage(
                RenderStage::RenderResource,
                poly_line_resource_provider_system.system(),
            )
            .add_system_to_stage(
                RenderStage::Draw,
                poly_line_draw_render_pipelines_system.system(),
            );

        // Setup pipeline
        let world = app.world_mut().cell();
        let mut shaders = world.get_resource_mut::<Assets<Shader>>().unwrap();
        let mut pipelines = world
            .get_resource_mut::<Assets<PipelineDescriptor>>()
            .unwrap();
        pipelines.set_untracked(
            POLY_LINE_PIPELINE_HANDLE,
            pipeline::build_poly_line_pipeline(&mut shaders),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn poly_line_draw_render_pipelines_system(
    mut draw_context: DrawContext,
    mut render_resource_bindings: ResMut<RenderResourceBindings>,
    msaa: Res<Msaa>,
    mut query: Query<
        (&mut Draw, &mut RenderPipelines, &PolyLine, &Visible),
        Without<OutsideFrustum>,
    >,
) {
    for (mut draw, mut render_pipelines, poly_line, visible) in query.iter_mut() {
        if !visible.is_visible {
            continue;
        }

        // set dynamic bindings
        let render_pipelines = &mut *render_pipelines;
        for render_pipeline in render_pipelines.pipelines.iter_mut() {
            render_pipeline.specialization.sample_count = msaa.samples;

            // TODO Consider moving to build_poly_line_pipeline
            // Needed to pass compiler check for all vertex buffer attibutes
            render_pipeline.specialization.vertex_buffer_layout = VertexBufferLayout {
                name: "PolyLine".into(),
                stride: 12,
                // But this field is overwritten
                step_mode: InputStepMode::Instance,
                attributes: vec![
                    VertexAttribute {
                        name: "Instance_Point0".into(),
                        format: VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        name: "Instance_Point1".into(),
                        format: VertexFormat::Float32x3,
                        offset: 12,
                        shader_location: 1,
                    },
                ],
            };

            if render_pipeline.dynamic_bindings_generation
                != render_pipelines.bindings.dynamic_bindings_generation()
            {
                render_pipeline.specialization.dynamic_bindings = render_pipelines
                    .bindings
                    .iter_dynamic_bindings()
                    .map(|name| name.to_string())
                    .collect::<HashSet<String>>();
                render_pipeline.dynamic_bindings_generation =
                    render_pipelines.bindings.dynamic_bindings_generation();
                for (handle, _) in render_pipelines.bindings.iter_assets() {
                    if let Some(bindings) = draw_context
                        .asset_render_resource_bindings
                        .get_untyped(handle)
                    {
                        for binding in bindings.iter_dynamic_bindings() {
                            render_pipeline
                                .specialization
                                .dynamic_bindings
                                .insert(binding.to_string());
                        }
                    }
                }
            }
        }

        // draw for each pipeline
        for render_pipeline in render_pipelines.pipelines.iter_mut() {
            let render_resource_bindings = &mut [
                &mut render_pipelines.bindings,
                &mut render_resource_bindings,
            ];
            draw_context
                .set_pipeline(
                    &mut draw,
                    &render_pipeline.pipeline,
                    &render_pipeline.specialization,
                )
                .unwrap();
            draw_context
                .set_bind_groups_from_bindings(&mut draw, render_resource_bindings)
                .unwrap();
            draw_context
                .set_vertex_buffers_from_bindings(&mut draw, &[&render_pipelines.bindings])
                .unwrap();

            // TODO line list
            // for line strip
            draw.draw(0..6, 0..(poly_line.vertices.len() - 1) as u32)
        }
    }
}

pub fn poly_line_resource_provider_system(
    render_resource_context: Res<Box<dyn RenderResourceContext>>,
    mut query: Query<(Entity, &PolyLine, &mut RenderPipelines), Changed<PolyLine>>,
) {
    // let mut changed_meshes = HashSet::default();
    let render_resource_context = &**render_resource_context;

    query.for_each_mut(|(entity, poly_line, mut render_pipelines)| {
        // remove previous buffer
        if let Some(buffer_id) = render_pipelines.bindings.vertex_attribute_buffer {
            render_resource_context.remove_buffer(buffer_id);
        }

        let buffer_id = render_resource_context.create_buffer_with_data(
            BufferInfo {
                size: poly_line.vertices.byte_len(),
                buffer_usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            poly_line.vertices.as_bytes(),
        );

        render_pipelines
            .bindings
            .vertex_attribute_buffer
            .replace(buffer_id);
    });
}

#[derive(Default, Reflect)]
#[reflect(Component)]
pub struct PolyLine {
    pub vertices: Vec<Vec3>,
}

#[derive(Default, Reflect)]
#[reflect(Component)]
pub struct PolyLineMaterial {}

#[derive(Bundle)]
pub struct PolyLineBundle {
    pub material: PolyLineMaterial,
    pub poly_line: PolyLine,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub visible: Visible,
    pub draw: Draw,
    pub render_pipelines: RenderPipelines,
    pub main_pass: MainPass,
}

impl Default for PolyLineBundle {
    fn default() -> Self {
        Self {
            material: PolyLineMaterial {},
            poly_line: PolyLine::default(),
            transform: Transform::default(),
            global_transform: GlobalTransform::default(),
            visible: Visible::default(),
            draw: Draw::default(),
            render_pipelines: RenderPipelines::from_handles(vec![
                &POLY_LINE_PIPELINE_HANDLE.typed()
            ]),
            main_pass: MainPass,
        }
    }
}