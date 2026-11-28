//! Under water with ray tracing: the ray traced lighting (Solari) leaves out
//! the distance fog every other tier closes in with under water, so the
//! sea floor came out clear and bright to the edge of the murk ball. This
//! pass lays the water's murk over the picture after the lighting, by how
//! far away each pixel is in the depth buffer (and no further than the
//! water's surface above), as the fog would have. Out of the water it does
//! the same for the weather's fog (`Gloom`), so rain and fog close in on
//! Ultra as they do on the other tiers.

use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin, UniformComponentPlugin};
use bevy::render::render_resource::binding_types::{texture_depth_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, LoadOp, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages, ShaderType, StoreOp, TextureFormat,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewDepthStencilTexture, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use std::collections::HashMap;

/// The murk on the main camera, while it's under water with ray tracing on.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
#[extract_app(RenderApp)]
pub struct Murk {
    /// The fog's colour (linear), and how much of it at its thickest.
    pub colour: Vec4,
    /// Where the murk starts, where it's whole, the camera's near plane,
    /// and how far the water's surface is above the eye.
    pub reach: Vec4,
    /// The view's half width and half height one unit ahead of the eye.
    pub ray: Vec4,
    pub right: Vec4,
    pub up: Vec4,
    pub back: Vec4,
}

const SHADER: Handle<Shader> = bevy::asset::uuid_handle!("3b8e5f0c-1d24-4a7e-9c61-5f2a7d8e03b9");

pub fn add(app: &mut App) {
    bevy::asset::load_internal_asset!(app, SHADER, "murk.wgsl", Shader::from_wgsl);
    app.add_plugins((ExtractComponentPlugin::<Murk>::default(), UniformComponentPlugin::<Murk>::default()));
    let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
    render
        .add_systems(RenderStartup, init)
        .add_systems(Render, (pipelines.in_set(RenderSystems::Prepare), bind_groups.in_set(RenderSystems::PrepareBindGroups)))
        .add_systems(Core3d, pass.after(Core3dSystems::MainPass).before(Core3dSystems::EarlyPostProcess));
}

#[derive(Resource)]
struct MurkPipeline {
    layout: BindGroupLayoutDescriptor,
    /// A pipeline for each format views draw in.
    by_format: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
struct ViewMurk {
    pipeline: CachedRenderPipelineId,
    bind_group: BindGroup,
}

fn init(mut commands: Commands) {
    let layout = BindGroupLayoutDescriptor::new("murk_layout", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_depth_2d(), uniform_buffer::<Murk>(true))));
    commands.insert_resource(MurkPipeline { layout, by_format: HashMap::new() });
}

fn pipelines(mut pipe: ResMut<MurkPipeline>, cache: Res<PipelineCache>, fullscreen: Res<FullscreenShader>, views: Query<&ExtractedView, With<Murk>>) {
    for view in &views {
        let format = view.target_format;
        if pipe.by_format.contains_key(&format) {
            continue;
        }
        let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("murk_pipeline".into()),
            layout: vec![pipe.layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: SHADER,
                targets: vec![Some(ColorTargetState { format, blend: Some(BlendState::ALPHA_BLENDING), write_mask: ColorWrites::ALL })],
                ..default()
            }),
            ..default()
        });
        pipe.by_format.insert(format, id);
    }
}

fn bind_groups(
    mut commands: Commands,
    pipe: Res<MurkPipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    uniforms: Res<ComponentUniforms<Murk>>,
    views: Query<(Entity, &ExtractedView, &ViewDepthStencilTexture), With<Murk>>,
    out: Query<Entity, (With<ViewMurk>, Without<Murk>)>,
) {
    for e in &out {
        commands.entity(e).remove::<ViewMurk>();
    }
    let Some(binding) = uniforms.uniforms().binding() else { return };
    for (e, view, depth) in &views {
        let (Some(&pipeline), Some(depth)) = (pipe.by_format.get(&view.target_format), depth.attachment.depth_stencil_views().depth_only_view()) else {
            continue;
        };
        let bind_group = device.create_bind_group("murk_bind_group", &cache.get_bind_group_layout(&pipe.layout), &BindGroupEntries::sequential((depth, binding.clone())));
        commands.entity(e).insert(ViewMurk { pipeline, bind_group });
    }
}

fn pass(view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<Murk>, &ViewMurk)>, cache: Res<PipelineCache>, mut ctx: RenderContext) {
    let (target, index, murk) = view.into_inner();
    let Some(pipeline) = cache.get_render_pipeline(murk.pipeline) else { return };
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("murk"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target.main_texture_view(),
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Load, store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &murk.bind_group, &[index.index()]);
    pass.draw(0..3, 0..1);
}

/// How thick the weather's fog lies over a ray traced picture: none on a
/// clear day (the atmosphere's own haze is enough there), most of the
/// other tiers' in rain, and whole in thick fog.
pub(crate) fn weather_murk(gloom: &super::Gloom) -> f32 {
    use crate::noise::smooth;
    0.65 * smooth(0.0, 0.15, gloom.fog) + 0.35 * smooth(0.0, 0.5, gloom.fog)
}

/// Puts the murk on the camera while it's under water with ray tracing on,
/// or out in the weather's fog, in the fog's colour (`super::sky` sets it),
/// and takes it off after.
#[allow(clippy::too_many_arguments)]
pub fn follow(
    mut commands: Commands,
    me: Res<crate::player::Me>,
    rt: Res<super::Raytraced>,
    voxels: Res<crate::world::Voxels>,
    weather: Res<crate::sky::Weather>,
    settings: Res<crate::settings::Settings>,
    cam: Query<(Entity, &Transform, &Projection, &DistanceFog, Has<Murk>), With<crate::player::MainCamera>>,
) {
    let Ok((e, t, projection, fog, has)) = cam.single() else { return };
    let Projection::Perspective(p) = projection else { return };
    let thick = if me.under_water { 1.0 } else { weather_murk(&super::Gloom::of(&weather, settings.reduce_flashing)) };
    if !rt.0 || thick < 0.005 {
        if has {
            commands.entity(e).remove::<Murk>();
        }
        return;
    }
    let half = (p.fov * 0.5).tan();
    // out of the water it reaches up into the sky
    let surface = if me.under_water { (crate::player::surface(&voxels, t.translation) - t.translation.y).max(0.0) } else { 1.0e9 };
    let reach = match fog.falloff {
        FogFalloff::Linear { start, end } => Vec2::new(start, end),
        _ => Vec2::new(1.0, super::UNDER_SEA),
    };
    let c = fog.color.to_linear();
    commands.entity(e).insert(Murk {
        colour: Vec4::new(c.red, c.green, c.blue, thick),
        reach: Vec4::new(reach.x, reach.y, p.near, surface),
        ray: Vec4::new(half * p.aspect_ratio, half, 0.0, 0.0),
        right: t.right().as_vec3().extend(0.0),
        up: t.up().as_vec3().extend(0.0),
        back: t.back().as_vec3().extend(0.0),
    });
}
