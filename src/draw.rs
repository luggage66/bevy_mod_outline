use bevy::pbr::{DrawMesh, SetMeshBindGroup, SetMeshViewBindGroup};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{DrawFunctions, RenderPhase, SetItemPipeline};
use bevy::render::render_resource::{PipelineCache, SpecializedMeshPipelines};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::{ExtractedView, RenderLayers};
use wgpu_types::Backend;

use crate::node::{OpaqueOutline, StencilOutline, TransparentOutline};
use crate::pipeline::{OutlinePipeline, PassType, PipelineKey};
use crate::uniforms::{
    DepthMode, OutlineFragmentUniform, OutlineStencilFlags, OutlineStencilUniform,
    OutlineVolumeFlags, OutlineVolumeUniform, SetOutlineStencilBindGroup,
    SetOutlineVolumeBindGroup,
};
use crate::view_uniforms::SetOutlineViewBindGroup;
use crate::OutlineRenderLayers;

pub(crate) type DrawStencil = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshBindGroup<1>,
    SetOutlineViewBindGroup<2>,
    SetOutlineStencilBindGroup<3>,
    DrawMesh,
);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn queue_outline_stencil_mesh(
    stencil_draw_functions: Res<DrawFunctions<StencilOutline>>,
    stencil_pipeline: Res<OutlinePipeline>,
    msaa: Res<Msaa>,
    mut pipelines: ResMut<SpecializedMeshPipelines<OutlinePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    render_meshes: Res<RenderAssets<Mesh>>,
    adapter_info: Res<RenderAdapterInfo>,
    material_meshes: Query<(
        Entity,
        &Handle<Mesh>,
        &OutlineStencilUniform,
        &OutlineStencilFlags,
        &OutlineRenderLayers,
    )>,
    mut views: Query<(
        &ExtractedView,
        &mut RenderPhase<StencilOutline>,
        Option<&RenderLayers>,
    )>,
) {
    let draw_stencil = stencil_draw_functions
        .read()
        .get_id::<DrawStencil>()
        .unwrap();

    let base_key = PipelineKey::new()
        .with_msaa(*msaa)
        .with_pass_type(PassType::Stencil)
        .with_opengl_workaround(adapter_info.0.backend == Backend::Gl);

    for (view, mut stencil_phase, view_mask) in views.iter_mut() {
        let rangefinder = view.rangefinder3d();
        let view_mask = view_mask.copied().unwrap_or_default();
        for (entity, mesh_handle, stencil_uniform, stencil_flags, outline_mask) in
            material_meshes.iter()
        {
            if !view_mask.intersects(outline_mask) {
                continue; // Layer not enabled
            }
            if stencil_flags.depth_mode == DepthMode::Invalid {
                continue; // DepthMode not propagated
            }
            if let Some(mesh) = render_meshes.get(mesh_handle) {
                let key = base_key
                    .with_primitive_topology(mesh.primitive_topology)
                    .with_depth_mode(stencil_flags.depth_mode)
                    .with_offset_zero(stencil_uniform.offset == 0.0);
                let pipeline = pipelines
                    .specialize(&pipeline_cache, &stencil_pipeline, key, &mesh.layout)
                    .unwrap();
                let distance =
                    rangefinder.distance(&Mat4::from_translation(stencil_uniform.origin));
                stencil_phase.add(StencilOutline {
                    entity,
                    pipeline,
                    draw_function: draw_stencil,
                    distance,
                });
            }
        }
    }
}

pub(crate) type DrawOutline = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshBindGroup<1>,
    SetOutlineViewBindGroup<2>,
    SetOutlineVolumeBindGroup<3>,
    DrawMesh,
);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn queue_outline_volume_mesh(
    opaque_draw_functions: Res<DrawFunctions<OpaqueOutline>>,
    transparent_draw_functions: Res<DrawFunctions<TransparentOutline>>,
    outline_pipeline: Res<OutlinePipeline>,
    msaa: Res<Msaa>,
    mut pipelines: ResMut<SpecializedMeshPipelines<OutlinePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    render_meshes: Res<RenderAssets<Mesh>>,
    adapter_info: Res<RenderAdapterInfo>,
    material_meshes: Query<(
        Entity,
        &Handle<Mesh>,
        &OutlineVolumeUniform,
        &OutlineVolumeFlags,
        &OutlineFragmentUniform,
        &OutlineRenderLayers,
    )>,
    mut views: Query<(
        &ExtractedView,
        &mut RenderPhase<OpaqueOutline>,
        &mut RenderPhase<TransparentOutline>,
        Option<&RenderLayers>,
    )>,
) {
    let draw_opaque_outline = opaque_draw_functions
        .read()
        .get_id::<DrawOutline>()
        .unwrap();
    let draw_transparent_outline = transparent_draw_functions
        .read()
        .get_id::<DrawOutline>()
        .unwrap();

    let base_key = PipelineKey::new()
        .with_msaa(*msaa)
        .with_opengl_workaround(adapter_info.0.backend == Backend::Gl);

    for (view, mut opaque_phase, mut transparent_phase, view_mask) in views.iter_mut() {
        let view_mask = view_mask.copied().unwrap_or_default();
        let rangefinder = view.rangefinder3d();
        for (entity, mesh_handle, volume_uniform, volume_flags, fragment_uniform, outline_mask) in
            material_meshes.iter()
        {
            if !view_mask.intersects(outline_mask) {
                continue; // Layer not enabled
            }
            if volume_flags.depth_mode == DepthMode::Invalid {
                continue; // DepthMode not propagated
            }
            if let Some(mesh) = render_meshes.get(mesh_handle) {
                let transparent = fragment_uniform.colour[3] < 1.0;
                let key = base_key
                    .with_primitive_topology(mesh.primitive_topology)
                    .with_pass_type(if transparent {
                        PassType::Transparent
                    } else {
                        PassType::Opaque
                    })
                    .with_depth_mode(volume_flags.depth_mode)
                    .with_offset_zero(volume_uniform.offset == 0.0)
                    .with_hdr_format(view.hdr);
                let pipeline = pipelines
                    .specialize(&pipeline_cache, &outline_pipeline, key, &mesh.layout)
                    .unwrap();
                let distance = rangefinder.distance(&Mat4::from_translation(volume_uniform.origin));
                if transparent {
                    transparent_phase.add(TransparentOutline {
                        entity,
                        pipeline,
                        draw_function: draw_transparent_outline,
                        distance,
                    });
                } else {
                    opaque_phase.add(OpaqueOutline {
                        entity,
                        pipeline,
                        draw_function: draw_opaque_outline,
                        distance,
                    });
                }
            }
        }
    }
}
