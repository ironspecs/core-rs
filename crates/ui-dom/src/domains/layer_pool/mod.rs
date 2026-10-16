//! Shared retained layer-pool boundary for `ui-dom`.
//!
//! This module exists so retained layer ownership, render execution support, and helper types stay under one explicit folder boundary instead of drifting back into one giant mixed-responsibility file.
//!
//! The folder boundary separates retained policy and owner types into
//! type-named files while keeping shared helpers, tests, and GPU plumbing in
//! one module owner.
//! Main-world ownership stays centered on `LayerPool`, render-world execution stays centered on `LayerPoolRenderExecutor`, and internal helper files stay private unless the domain truly exports them.
//!
//! Staged paint ops are owned by `domains/render_ops` and arrive at the executor
//! through the registered-handler drain at the end of extract.
//!
//! Retained sizing policy:
//!
//! - allocate exact to the current retained need
//! - reuse the current retained target while it stays compatible
//! - grow exact when retained need exceeds the current target
//! - do not shrink later when retained need becomes smaller
//! - expect caller-side coalescing to avoid resize thrash
//!
//! Hard boundary rules for retained-op execution:
//!
//! - `LayerPool` owns retained surface lifetime and retained mapping, not a
//!   duplicated retained-op graph.
//! - `LayerPool` and `LayerLease` must not own, consume, store, or duplicate
//!   authored retained ops.
//! - Do not store a second translated retained plan inside `LayerPool`.
//! - Do not introduce per-op allocation in the hot path.
//! - Do not introduce a hot-path scratch translated-op buffer.
//! - Do not introduce a CPU rasterization fallback, CPU-side retained render
//!   cache, or mirrored per-layer CPU render payload.
//! - Do not introduce a fresh final GPU instance `Vec` allocation in the hot
//!   path.
//! - Authored ops stay owned outside `LayerPool`; this module supplies retained
//!   mapping and target only.
//! - Translation must happen only at the final GPU submission boundary.
//! - Do not build CPU-side intermediate retained-op or instance collections.
//! - The only allowed retained-op execution shape is `ops -> GPU buffer ->
//!   shader`.
//! - One fixed shader-consumed GPU op-buffer contract must be chosen for the
//!   retained execution boundary; do not leave it flexible or per-path.
//! - Retained ops must be encoded into a GPU buffer and consumed by shader code
//!   directly.
//! - Do not expand retained ops on the CPU into rect instances, quads, draw
//!   lists, or any other intermediate render payload.
//! - Retained-op application must use the stored layer mapping directly only at
//!   the final GPU-facing boundary while encoding the GPU op buffer.
//! - Do not add compatibility shims or adapter layers that translate CPU-side
//!   retained render payloads into the GPU op-buffer path.

mod external_layer_target;
mod font_registration_state;
mod frozen_layer_config;
mod layer_config;
mod layer_lease;
#[allow(clippy::module_inception)]
mod layer_pool;
mod layer_pool_apply_error;
mod layer_pool_metric_name;
mod layer_pool_metrics;
mod layer_pool_policy;
mod layer_pool_render_executor;
mod layer_pool_render_feedback;
mod layer_pool_render_job;
mod layer_pool_render_job_queue;
mod layer_pool_render_state;
mod layer_pool_text_render;
mod preload_warmup_queue;
mod text_run_render_state;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use bevy::{
    asset::{AssetId, AssetServer, Assets, LoadState, RenderAssetUsages},
    image::Image,
    math::UVec2,
    prelude::{Handle, Resource, World},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    render::{
        MainWorld,
        render_asset::RenderAssets,
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        texture::GpuImage,
    },
};
use bytemuck::{Pod, Zeroable};
use collections::lru_cache::LruCache;
use collections::metrics::{MetricName, MetricSnapshot, Metrics};
use cosmic_text::{
    Buffer, CacheKey, FontSystem, LayoutGlyph, PhysicalGlyph, SwashCache, SwashContent,
};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use wgpu;

use crate::bevy::icon_assets::{
    RequestedUiIconAtlasAssets, UiIconAtlasManifestAsset, requested_ui_icon_atlas_assets,
};
use crate::domains::{
    icon_atlas::{
        ValidatedUiIconAtlasEntry, ValidatedUiIconAtlasManifest, alpha_mask_bytes_from_image,
        icon_density_from_pixel_density, validate_manifest_asset,
    },
    layer_ops::{
        IconPaintOp, TextAlign, TextFontFamilyId, TextLanguageId, TextRunId, TextRunSignature,
        TextWrapMode,
    },
    layer_ops::{ImagePaintOp, SvgPaintOp, TextRunPaintOp},
    paint::RectPaintOp,
    render_ops::{RenderOpsHandlers, StagedLayerApply},
    text_layout,
};
use crate::numeric::{f32_to_u32, i32_to_f32, u32_to_f32};
use crate::render_plugin::{
    UiDomFontAsset, UiDomRenderSupport, UiDomTextFontCatalog, UiDomTextFontExtractState,
};
use crate::types::{DomWhiteSpace, PixelDensity};
use layer_config::FrozenLayerConfig;
pub use layer_config::LayerConfig;
use layer_pool_metrics::LayerPoolMetricStore;
use layer_pool_text_render::{
    TextRunRenderState, TextRunSurfaceContext, create_text_run_render_state,
};

pub use layer_lease::LayerLease;
pub use layer_pool::LayerPaintTarget;
pub use layer_pool::LayerPool;
pub use layer_pool_apply_error::LayerPoolApplyError;
pub use layer_pool_metric_name::LayerPoolMetricName;
pub use layer_pool_metrics::LayerPoolMetricHistoryEntry;
pub use layer_pool_metrics::LayerPoolMetricHistoryIter;
pub use layer_pool_metrics::LayerPoolMetrics;
pub use layer_pool_policy::LayerPoolPolicy;

use layer_pool_render_feedback::LayerPoolRenderFeedback;
use layer_pool_render_job::LayerPoolRenderJob;
use layer_pool_render_state::RenderSurfaceApplyResult;

pub use external_layer_target::ExternalLayerTarget;
pub(crate) use layer_pool_render_executor::LayerPoolRenderExecutor;
pub(crate) use layer_pool_render_job_queue::LayerPoolRenderJobQueue;

#[cfg(test)]
use crate::domains::icon_atlas::ValidatedUiIconAtlasLookup;
#[cfg(test)]
use crate::domains::layer_ops::LayerPaintPlan;
#[cfg(test)]
use crate::domains::render_ops::{RenderOpsHandler, register_render_ops_handler};

const LAYER_POOL_HISTORY_CAPACITY: usize = 128;
const IMAGE_PLACEHOLDER_FILL_RGBA: [f32; 4] = [96.0 / 255.0, 168.0 / 255.0, 1.0, 1.0];
const UI_TEXT_BUFFER_CACHE_CAPACITY: usize = 128;
const UI_TEXT_GLYPH_CACHE_CAPACITY: usize = 1024;
const UI_TEXT_RUN_SIGNATURE_CACHE_CAPACITY: usize = 1024;
const UI_TEXT_PRELOAD_BATCH_SIZE: usize = 8;
const UI_TEXT_PRELOAD_FONT_SIZES_PX: [f32; 10] =
    [10.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0, 24.0, 28.0];
const UI_TEXT_ENGLISH_WARMUP_SAMPLES: [&str; 10] = [
    "Warm UI Text",
    "Scene",
    "Output",
    "Connect",
    "Control",
    "Preview",
    "Settings",
    "Library",
    "Profile",
    "Broadcast",
];
const UI_TEXT_JAPANESE_WARMUP_SAMPLES: [&str; 10] = [
    "メニュー",
    "シーン",
    "出力",
    "接続",
    "制御",
    "プレビュー",
    "設定",
    "ライブラリ",
    "レイヤー",
    "配信",
];
const UI_TEXT_SIMPLIFIED_CHINESE_WARMUP_SAMPLES: [&str; 10] = [
    "颜色", "节点", "连接", "预览", "设置", "控制", "布局", "图层", "输出", "直播",
];
const UI_TEXT_KOREAN_WARMUP_SAMPLES: [&str; 10] = [
    "메뉴",
    "장면",
    "출력",
    "연결",
    "제어",
    "미리보기",
    "설정",
    "라이브러리",
    "레이어",
    "방송",
];
const UI_TEXT_THAI_WARMUP_SAMPLES: [&str; 10] = [
    "เมนู",
    "ฉาก",
    "เอาต์พุต",
    "เชื่อมต่อ",
    "ควบคุม",
    "แสดงตัวอย่าง",
    "ตั้งค่า",
    "คลัง",
    "เลเยอร์",
    "สตรีม",
];

new_key_type! {
    struct LayerId;
}

type LayerGeneration = u32;

#[derive(Debug)]
struct LayerRecord {
    generation: LayerGeneration,
    policy: LayerPoolPolicy,
    image_width_px: u32,
    image_height_px: u32,
    layer_zero_x_in_image_px: f32,
    layer_zero_y_in_image_px: f32,
    image: Option<Handle<Image>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_field_names)]
struct LayerMapping {
    image_width_px: f32,
    image_height_px: f32,
    layer_zero_x_in_image_px: f32,
    layer_zero_y_in_image_px: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LayerRect {
    origin_x: f32,
    origin_y: f32,
    width: f32,
    height: f32,
}

struct LayerPoolRenderState {
    rect: LayerPipelineState,
    icon: IconRenderState,
    text_run: TextRunRenderState,
    svg: LayerPipelineState,
    image: LayerPipelineState,
}

struct LayerPipelineState {
    pipeline: wgpu::RenderPipeline,
    gpu_buffer: wgpu::Buffer,
    gpu_capacity: usize,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct IconRenderState {
    pipeline: wgpu::RenderPipeline,
    gpu_buffer: wgpu::Buffer,
    gpu_capacity: usize,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    atlas_bind_group_layout: wgpu::BindGroupLayout,
    atlas_sampler: wgpu::Sampler,
    atlas: Option<RenderOwnedIconAtlas>,
}

enum IconAtlasLoadState {
    Empty,
    Loading(LoadingIconAtlasState),
    Ready,
    Failed,
}

struct LoadingIconAtlasState {
    requested_assets: RequestedUiIconAtlasAssets,
    validated_manifest: Option<ValidatedUiIconAtlasManifest>,
    pending_install: Option<PendingIconAtlasInstall>,
}

struct PendingIconAtlasInstall {
    validated_manifest: ValidatedUiIconAtlasManifest,
    alpha_mask_bytes: Arc<[u8]>,
    atlas_width_px: u32,
    atlas_height_px: u32,
}

struct RenderOwnedIconAtlas {
    _texture: wgpu::Texture,
    _texture_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    manifest: ValidatedUiIconAtlasManifest,
}

#[derive(Clone, Debug)]
struct ExtractedFontAssetRecord {
    asset_id: AssetId<UiDomFontAsset>,
    bytes: std::sync::Arc<[u8]>,
}

#[derive(Clone, Debug)]
struct ExtractedTextRunRecord {
    text_run_id: TextRunId,
    text: std::sync::Arc<str>,
    language_hint_id: TextLanguageId,
    font_family_id: TextFontFamilyId,
}

#[derive(Clone, Debug)]
struct RenderTextRunRecord {
    text: std::sync::Arc<str>,
    language_hint_id: TextLanguageId,
    font_family_id: TextFontFamilyId,
}

#[derive(Clone, Debug)]
struct CanonicalTextRunRecord {
    signature: TextRunSignature,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PendingLeaseDependencies {
    missing_text_runs: Vec<TextRunId>,
    missing_font_families: Vec<TextFontFamilyId>,
}

fn external_rgba_byte_len(image_extent_px: UVec2) -> usize {
    usize::try_from(image_extent_px.x)
        .expect("external upload width should fit usize")
        .checked_mul(
            usize::try_from(image_extent_px.y).expect("external upload height should fit usize"),
        )
        .and_then(|value| value.checked_mul(4))
        .expect("external upload byte length should fit usize")
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
#[allow(clippy::struct_field_names)]
struct GpuRectPipelineUniform {
    target_size_px: [f32; 4],
    layer_zero_in_image_px: [f32; 4],
    density_scale_px: [f32; 4],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct GpuTextRunPipelineUniform {
    target_size_px: [f32; 4],
    layer_zero_in_image_px: [f32; 4],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
#[allow(clippy::struct_field_names)]
struct GpuIconPipelineUniform {
    target_size_px: [f32; 4],
    layer_zero_in_image_px: [f32; 4],
    density_scale_px: [f32; 4],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct GpuDebugRectOp {
    position_px: [f32; 2],
    size_px: [f32; 2],
    fill_rgba: [f32; 4],
}

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct GpuIconOp {
    position_px: [f32; 2],
    size_px: [f32; 2],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    color_rgba: [f32; 4],
}

#[derive(Clone, Copy)]
struct PipelineLabels {
    shader: &'static str,
    bind_group_layout: &'static str,
    pipeline_layout: &'static str,
    render_pipeline: &'static str,
    uniform_buffer: &'static str,
    bind_group: &'static str,
    instance_buffer: &'static str,
    encoder: &'static str,
    render_pass: &'static str,
}

const DIRECT_RECT_SHADER: &str = include_str!("shaders/layer_pool_rect.wgsl");

const DIRECT_TEXT_RUN_SHADER: &str = include_str!("shaders/layer_pool_text_run.wgsl");
const DIRECT_TEXT_DEBUG_SHADER: &str = include_str!("shaders/layer_pool_text_debug.wgsl");
const DIRECT_ICON_SHADER: &str = include_str!("shaders/layer_pool_icon.wgsl");
const DIRECT_SVG_SHADER: &str = include_str!("shaders/layer_pool_svg.wgsl");
const DIRECT_IMAGE_SHADER: &str = include_str!("shaders/layer_pool_image.wgsl");

const GPU_RECT_OP_VEC2_SIZE: wgpu::BufferAddress = 8;
const GPU_RECT_OP_VEC4_SIZE: wgpu::BufferAddress = 16;
const GPU_RECT_OP_ATTRIBUTES: [wgpu::VertexAttribute; 9] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GPU_RECT_OP_VEC2_SIZE,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 2,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 3,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 4,
        shader_location: 5,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 5,
        shader_location: 6,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 6,
        shader_location: 7,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 7,
        shader_location: 8,
    },
];

const GPU_DEBUG_RECT_OP_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GPU_RECT_OP_VEC2_SIZE,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE,
        shader_location: 2,
    },
];

const GPU_ICON_OP_ATTRIBUTES: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GPU_RECT_OP_VEC2_SIZE,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GPU_RECT_OP_VEC4_SIZE,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GPU_RECT_OP_VEC4_SIZE + GPU_RECT_OP_VEC2_SIZE,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE * 2,
        shader_location: 4,
    },
];

#[derive(Clone, Copy, Debug, Pod, Zeroable)]
#[repr(C)]
struct GpuTextRunOp {
    position_px: [f32; 2],
    size_px: [f32; 2],
    color_rgba: [f32; 4],
}

const GPU_TEXT_RUN_OP_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: GPU_RECT_OP_VEC2_SIZE,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: GPU_RECT_OP_VEC4_SIZE,
        shader_location: 2,
    },
];

impl LayerRect {
    fn from_tuple(layer_rect_px: (f32, f32, f32, f32)) -> Self {
        Self {
            origin_x: layer_rect_px.0,
            origin_y: layer_rect_px.1,
            width: layer_rect_px.2,
            height: layer_rect_px.3,
        }
    }
}

impl LayerPoolRenderState {
    fn new(device: &RenderDevice, layer_config: FrozenLayerConfig) -> Self {
        Self {
            rect: create_rect_pipeline_state(device),
            icon: create_icon_render_state(device),
            text_run: create_text_run_render_state(device, layer_config.pixel_density()),
            svg: create_debug_rect_pipeline_state(device, "svg", DIRECT_SVG_SHADER),
            image: create_debug_rect_pipeline_state(device, "image", DIRECT_IMAGE_SHADER),
        }
    }
}

impl IconAtlasLoadState {
    pub(super) fn take_pending_install(&mut self) -> Option<PendingIconAtlasInstall> {
        let IconAtlasLoadState::Loading(loading_state) = self else {
            return None;
        };
        loading_state.pending_install.take()
    }
}

impl LayerPoolRenderState {
    pub(super) fn apply_plan_to_surface(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        gpu_image: &GpuImage,
        pixel_density: PixelDensity,
        apply: &StagedLayerApply,
    ) -> RenderSurfaceApplyResult {
        let rect_buffer_grew = apply_rects_to_surface(
            &mut self.rect,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.rects,
            apply.layer_zero_in_image_px,
        );
        apply_icons_to_surface(
            &mut self.icon,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.icons,
            apply.layer_zero_in_image_px,
        );
        let text_runs_ready = self.text_run.apply_to_surface(
            &TextRunSurfaceContext {
                device,
                queue,
                gpu_image,
                layer_zero_in_image_px: apply.layer_zero_in_image_px,
                lease: apply.lease,
            },
            &apply.plan.text_runs,
        );
        apply_svgs_to_surface(
            &mut self.svg,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.svgs,
            apply.layer_zero_in_image_px,
        );
        apply_images_to_surface(
            &mut self.image,
            device,
            queue,
            gpu_image,
            pixel_density,
            &apply.plan.images,
            apply.layer_zero_in_image_px,
        );
        RenderSurfaceApplyResult {
            rect_buffer_grew,
            text_runs_ready,
        }
    }

    pub(super) fn install_ready_icon_atlas(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        pending_install: PendingIconAtlasInstall,
    ) {
        install_ready_icon_atlas(device, queue, &mut self.icon, pending_install);
    }

    pub(super) fn icon_atlas_is_ready(&self) -> bool {
        self.icon.atlas.is_some()
    }
}

pub(crate) fn extract_layer_pool_render_work(
    mut main_world: bevy::prelude::ResMut<MainWorld>,
    mut executor: bevy::prelude::ResMut<LayerPoolRenderExecutor>,
) {
    flush_layer_pool_render_feedback(&mut main_world, &mut executor.feedback);
    extract_text_font_assets(&mut main_world, &mut executor);
    poll_icon_atlas_assets(&mut main_world, &mut executor);
    if main_world.contains_resource::<LayerPool>() {
        main_world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            for text_run_change in layer_pool.drain_pending_text_run_changes() {
                executor.apply_text_run_change(text_run_change);
            }
            for text_run_id in layer_pool.drain_pending_text_run_removals() {
                executor.apply_text_run_removal(text_run_id);
            }
        });
    }
    main_world.init_resource::<LayerPoolRenderJobQueue>();
    main_world.resource_scope(
        |_, mut pending_jobs: bevy::prelude::Mut<LayerPoolRenderJobQueue>| {
            for job in pending_jobs.jobs.drain(..) {
                executor.apply_job(job);
            }
        },
    );
    main_world.init_resource::<RenderOpsHandlers>();
    for handler in &main_world.resource::<RenderOpsHandlers>().0 {
        let batch = handler.take_staged();
        for apply in batch.applies {
            executor.stage_apply(apply);
        }
        for upload in batch.external_uploads {
            executor.stage_external_rgba_upload(upload);
        }
    }
}

fn extract_text_font_assets(main_world: &mut MainWorld, executor: &mut LayerPoolRenderExecutor) {
    let Some(font_catalog) = main_world.get_resource::<UiDomTextFontCatalog>().cloned() else {
        return;
    };
    executor.set_required_font_assets(font_catalog.required_asset_ids_by_family());
    main_world.init_resource::<UiDomTextFontExtractState>();
    main_world.resource_scope(
        |world, mut extract_state: bevy::prelude::Mut<UiDomTextFontExtractState>| {
            let Some(font_assets) = world.get_resource::<Assets<UiDomFontAsset>>() else {
                return;
            };
            for handle in font_catalog.ui_sans_handles() {
                let asset_id = handle.id();
                if extract_state.already_extracted(asset_id) {
                    continue;
                }
                let Some(font_asset) = font_assets.get(handle) else {
                    continue;
                };
                executor.queue_font_asset_load(ExtractedFontAssetRecord {
                    asset_id,
                    bytes: font_asset.bytes.clone(),
                });
                extract_state.mark_extracted(asset_id);
            }
        },
    );
}

fn poll_icon_atlas_assets(main_world: &mut MainWorld, executor: &mut LayerPoolRenderExecutor) {
    request_icon_atlas_assets(main_world, executor);
    prepare_icon_atlas_install(main_world, executor);
}

fn request_icon_atlas_assets(main_world: &mut MainWorld, executor: &mut LayerPoolRenderExecutor) {
    if !matches!(executor.icon_atlas_state, IconAtlasLoadState::Empty) {
        return;
    }
    let Some(asset_server) = main_world.get_resource::<AssetServer>() else {
        return;
    };
    let retained_density = icon_density_from_pixel_density(executor.layer_config.pixel_density());
    executor.icon_atlas_state = IconAtlasLoadState::Loading(LoadingIconAtlasState {
        requested_assets: requested_ui_icon_atlas_assets(asset_server, retained_density),
        validated_manifest: None,
        pending_install: None,
    });
}

fn prepare_icon_atlas_install(main_world: &mut MainWorld, executor: &mut LayerPoolRenderExecutor) {
    let Some(asset_server) = main_world.get_resource::<AssetServer>() else {
        return;
    };
    let (requested_assets, validated_manifest_ready) = match &executor.icon_atlas_state {
        IconAtlasLoadState::Loading(loading_state) if loading_state.pending_install.is_none() => (
            loading_state.requested_assets.clone(),
            loading_state.validated_manifest.is_some(),
        ),
        _ => return,
    };
    if let Some((asset_kind, error)) =
        icon_atlas_asset_load_failure(asset_server, &requested_assets)
    {
        executor.icon_atlas_state = IconAtlasLoadState::Failed;
        panic!(
            "ui-dom render-owned icon atlas {asset_kind} should load for {}: {error:#}",
            requested_assets.retained_density.file_suffix(),
        );
    }
    if !validated_manifest_ready {
        let Some(manifest_asset) = icon_atlas_manifest_asset(main_world, &requested_assets) else {
            return;
        };
        let validated_manifest =
            validate_manifest_asset(manifest_asset, requested_assets.retained_density)
                .unwrap_or_else(|error| {
                    executor.icon_atlas_state = IconAtlasLoadState::Failed;
                    panic!(
                        "ui-dom render-owned icon atlas manifest should validate for {}: {error:#}",
                        requested_assets.retained_density.file_suffix(),
                    );
                });
        let IconAtlasLoadState::Loading(loading_state) = &mut executor.icon_atlas_state else {
            return;
        };
        loading_state.validated_manifest = Some(validated_manifest);
    }
    let pending_install = match pending_icon_atlas_install_from_assets(main_world, executor) {
        Ok(Some(pending_install)) => pending_install,
        Ok(None) => return,
        Err(error) => {
            executor.icon_atlas_state = IconAtlasLoadState::Failed;
            panic!(
                "ui-dom render-owned icon atlas image should decode for {}: {error:#}",
                requested_assets.retained_density.file_suffix(),
            );
        }
    };
    let IconAtlasLoadState::Loading(loading_state) = &mut executor.icon_atlas_state else {
        return;
    };
    loading_state.pending_install = Some(pending_install);
}

fn icon_atlas_asset_load_failure(
    asset_server: &AssetServer,
    requested_assets: &RequestedUiIconAtlasAssets,
) -> Option<(&'static str, Arc<bevy::asset::AssetLoadError>)> {
    match asset_server.load_state(requested_assets.manifest_handle.id()) {
        LoadState::Failed(error) => return Some(("manifest", error)),
        LoadState::NotLoaded | LoadState::Loading | LoadState::Loaded => {}
    }
    match asset_server.load_state(requested_assets.image_handle.id()) {
        LoadState::Failed(error) => Some(("image", error)),
        LoadState::NotLoaded | LoadState::Loading | LoadState::Loaded => None,
    }
}

fn icon_atlas_manifest_asset<'a>(
    main_world: &'a MainWorld,
    requested_assets: &RequestedUiIconAtlasAssets,
) -> Option<&'a UiIconAtlasManifestAsset> {
    let manifest_assets = main_world.get_resource::<Assets<UiIconAtlasManifestAsset>>()?;
    manifest_assets.get(&requested_assets.manifest_handle)
}

fn pending_icon_atlas_install_from_assets(
    main_world: &MainWorld,
    executor: &LayerPoolRenderExecutor,
) -> anyhow::Result<Option<PendingIconAtlasInstall>> {
    let IconAtlasLoadState::Loading(loading_state) = &executor.icon_atlas_state else {
        return Ok(None);
    };
    let Some(validated_manifest) = loading_state.validated_manifest.clone() else {
        return Ok(None);
    };
    let Some(images) = main_world.get_resource::<Assets<Image>>() else {
        return Ok(None);
    };
    let Some(image) = images.get(&loading_state.requested_assets.image_handle) else {
        return Ok(None);
    };
    Ok(Some(PendingIconAtlasInstall {
        validated_manifest,
        alpha_mask_bytes: alpha_mask_bytes_from_image(image)?,
        atlas_width_px: image.texture_descriptor.size.width,
        atlas_height_px: image.texture_descriptor.size.height,
    }))
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn apply_layer_pool_render_work(
    gpu_images: bevy::prelude::Res<RenderAssets<GpuImage>>,
    device: bevy::prelude::Res<RenderDevice>,
    queue: bevy::prelude::Res<RenderQueue>,
    mut executor: bevy::prelude::ResMut<LayerPoolRenderExecutor>,
) {
    executor.apply_pending(&gpu_images, &device, &queue);
}

fn apply_rects_to_surface(
    rect_pipeline: &mut LayerPipelineState,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu_image: &GpuImage,
    pixel_density: PixelDensity,
    rects: &[RectPaintOp],
    layer_zero_in_image_px: [f32; 2],
) -> bool {
    let density_scale = pixel_density_scale(pixel_density);
    let buffer_grew = ensure_gpu_capacity(
        device,
        rect_pipeline,
        rects.len(),
        std::mem::size_of::<RectPaintOp>(),
        "ui_dom_layer_pool_rect_instances",
    );
    queue.write_buffer(
        &rect_pipeline.uniform_buffer,
        0,
        bytemuck::bytes_of(&GpuRectPipelineUniform {
            target_size_px: [
                u32_to_f32(gpu_image.size.width),
                u32_to_f32(gpu_image.size.height),
                0.0,
                0.0,
            ],
            layer_zero_in_image_px: [
                layer_zero_in_image_px[0],
                layer_zero_in_image_px[1],
                0.0,
                0.0,
            ],
            density_scale_px: [density_scale, 0.0, 0.0, 0.0],
        }),
    );
    if !rects.is_empty() {
        queue.write_buffer(&rect_pipeline.gpu_buffer, 0, bytemuck::cast_slice(rects));
    }
    let instance_count =
        u32::try_from(rects.len()).expect("rect instance count should fit into u32");
    let mut encoder =
        device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ui_dom_layer_pool_rect_encoder"),
            });
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui_dom_layer_pool_rect_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &gpu_image.texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if instance_count > 0 {
            render_pass.set_pipeline(&rect_pipeline.pipeline);
            render_pass.set_bind_group(0, &rect_pipeline.bind_group, &[]);
            render_pass.set_vertex_buffer(0, rect_pipeline.gpu_buffer.slice(..));
            render_pass.draw(0..6, 0..instance_count);
        }
    }
    queue.submit(std::iter::once(encoder.finish()));
    buffer_grew
}

fn apply_svgs_to_surface(
    svg_pipeline: &mut LayerPipelineState,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu_image: &GpuImage,
    pixel_density: PixelDensity,
    svgs: &[SvgPaintOp],
    layer_zero_in_image_px: [f32; 2],
) -> bool {
    apply_debug_rect_ops_to_surface(
        svg_pipeline,
        &DebugRectSurfaceContext {
            device,
            queue,
            gpu_image,
            pixel_density,
            op_count: svgs.len(),
            layer_zero_in_image_px,
            pipeline_name: "svg",
        },
        |gpu_buffer, queue| {
            for (index, svg) in svgs.iter().enumerate() {
                write_debug_rect_op(
                    gpu_buffer,
                    queue,
                    index,
                    GpuDebugRectOp {
                        position_px: svg.position_px,
                        size_px: svg.size_px,
                        fill_rgba: svg.color_rgba,
                    },
                );
            }
        },
    )
}

fn apply_images_to_surface(
    image_pipeline: &mut LayerPipelineState,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu_image: &GpuImage,
    pixel_density: PixelDensity,
    images: &[ImagePaintOp],
    layer_zero_in_image_px: [f32; 2],
) -> bool {
    apply_debug_rect_ops_to_surface(
        image_pipeline,
        &DebugRectSurfaceContext {
            device,
            queue,
            gpu_image,
            pixel_density,
            op_count: images.len(),
            layer_zero_in_image_px,
            pipeline_name: "image",
        },
        |gpu_buffer, queue| {
            for (index, image) in images.iter().enumerate() {
                write_debug_rect_op(
                    gpu_buffer,
                    queue,
                    index,
                    GpuDebugRectOp {
                        position_px: image.position_px,
                        size_px: image.size_px,
                        fill_rgba: IMAGE_PLACEHOLDER_FILL_RGBA,
                    },
                );
            }
        },
    )
}

fn apply_icons_to_surface(
    icon_state: &mut IconRenderState,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu_image: &GpuImage,
    pixel_density: PixelDensity,
    icons: &[IconPaintOp],
    layer_zero_in_image_px: [f32; 2],
) -> bool {
    if icons.is_empty() {
        return false;
    }
    let buffer_grew = ensure_icon_gpu_capacity(device, icon_state, icons.len());
    let Some(installed_atlas) = icon_state.atlas.as_ref() else {
        panic!("render-owned icon atlas should exist when icon ops are present");
    };
    queue.write_buffer(
        &icon_state.uniform_buffer,
        0,
        bytemuck::bytes_of(&GpuIconPipelineUniform {
            target_size_px: [
                u32_to_f32(gpu_image.size.width),
                u32_to_f32(gpu_image.size.height),
                0.0,
                0.0,
            ],
            layer_zero_in_image_px: [
                layer_zero_in_image_px[0],
                layer_zero_in_image_px[1],
                0.0,
                0.0,
            ],
            density_scale_px: [pixel_density_scale(pixel_density), 0.0, 0.0, 0.0],
        }),
    );
    write_icon_ops(
        queue,
        &icon_state.gpu_buffer,
        icons,
        &installed_atlas.manifest,
    );
    let instance_count = u32::try_from(icons.len()).expect("icon instance count should fit u32");
    let mut encoder =
        device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ui_dom_layer_pool_icon_encoder"),
            });
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui_dom_layer_pool_icon_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &gpu_image.texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_pipeline(&icon_state.pipeline);
        render_pass.set_bind_group(0, &icon_state.uniform_bind_group, &[]);
        render_pass.set_bind_group(1, &installed_atlas.bind_group, &[]);
        render_pass.set_vertex_buffer(0, icon_state.gpu_buffer.slice(..));
        render_pass.draw(0..6, 0..instance_count);
    }
    queue.submit(std::iter::once(encoder.finish()));
    buffer_grew
}

fn write_icon_ops(
    queue: &RenderQueue,
    gpu_buffer: &wgpu::Buffer,
    icons: &[IconPaintOp],
    atlas_manifest: &ValidatedUiIconAtlasManifest,
) {
    for (index, icon) in icons.iter().enumerate() {
        write_icon_op(gpu_buffer, queue, index, gpu_icon_op(icon, atlas_manifest));
    }
}

fn gpu_icon_op(icon: &IconPaintOp, atlas_manifest: &ValidatedUiIconAtlasManifest) -> GpuIconOp {
    let icon_entry = atlas_manifest.lookup.entry(icon.icon_id);
    let (uv_min, uv_max) = icon_uv_window(icon_entry, atlas_manifest);
    GpuIconOp {
        position_px: icon.position_px,
        size_px: icon.size_px,
        uv_min,
        uv_max,
        color_rgba: icon.color_rgba,
    }
}

fn icon_uv_window(
    icon_entry: &ValidatedUiIconAtlasEntry,
    atlas_manifest: &ValidatedUiIconAtlasManifest,
) -> ([f32; 2], [f32; 2]) {
    let atlas_width_px = u32_to_f32(atlas_manifest.atlas_width_px);
    let atlas_height_px = u32_to_f32(atlas_manifest.atlas_height_px);
    let left_px = icon_entry.packed_rect_px[0];
    let top_px = icon_entry.packed_rect_px[1];
    let right_px = left_px + atlas_manifest.design_box_size_px;
    let bottom_px = top_px + atlas_manifest.design_box_size_px;
    (
        [
            u32_to_f32(left_px) / atlas_width_px,
            u32_to_f32(top_px) / atlas_height_px,
        ],
        [
            u32_to_f32(right_px) / atlas_width_px,
            u32_to_f32(bottom_px) / atlas_height_px,
        ],
    )
}

fn write_icon_op(gpu_buffer: &wgpu::Buffer, queue: &RenderQueue, index: usize, op: GpuIconOp) {
    let byte_offset = u64::try_from(index * std::mem::size_of::<GpuIconOp>())
        .expect("icon op write offset should fit u64");
    queue.write_buffer(gpu_buffer, byte_offset, bytemuck::bytes_of(&op));
}

struct DebugRectSurfaceContext<'a> {
    device: &'a RenderDevice,
    queue: &'a RenderQueue,
    gpu_image: &'a GpuImage,
    pixel_density: PixelDensity,
    op_count: usize,
    layer_zero_in_image_px: [f32; 2],
    pipeline_name: &'a str,
}

fn apply_debug_rect_ops_to_surface<F>(
    pipeline: &mut LayerPipelineState,
    context: &DebugRectSurfaceContext<'_>,
    write_ops: F,
) -> bool
where
    F: FnOnce(&wgpu::Buffer, &RenderQueue),
{
    if context.op_count == 0 {
        return false;
    }
    let labels = pipeline_labels(context.pipeline_name);
    let density_scale = pixel_density_scale(context.pixel_density);
    let buffer_grew = ensure_gpu_capacity(
        context.device,
        pipeline,
        context.op_count,
        std::mem::size_of::<GpuDebugRectOp>(),
        labels.instance_buffer,
    );
    context.queue.write_buffer(
        &pipeline.uniform_buffer,
        0,
        bytemuck::bytes_of(&GpuRectPipelineUniform {
            target_size_px: [
                u32_to_f32(context.gpu_image.size.width),
                u32_to_f32(context.gpu_image.size.height),
                0.0,
                0.0,
            ],
            layer_zero_in_image_px: [
                context.layer_zero_in_image_px[0],
                context.layer_zero_in_image_px[1],
                0.0,
                0.0,
            ],
            density_scale_px: [density_scale, 0.0, 0.0, 0.0],
        }),
    );
    write_ops(&pipeline.gpu_buffer, context.queue);
    let instance_count =
        u32::try_from(context.op_count).expect("debug rect instance count should fit into u32");
    let mut encoder =
        context
            .device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(labels.encoder),
            });
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(labels.render_pass),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &context.gpu_image.texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if instance_count > 0 {
            render_pass.set_pipeline(&pipeline.pipeline);
            render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
            render_pass.set_vertex_buffer(0, pipeline.gpu_buffer.slice(..));
            render_pass.draw(0..6, 0..instance_count);
        }
    }
    context.queue.submit(std::iter::once(encoder.finish()));
    buffer_grew
}

#[allow(clippy::too_many_lines)]
fn create_icon_render_state(device: &RenderDevice) -> IconRenderState {
    let wgpu_device = device.wgpu_device();
    let labels = icon_pipeline_labels();
    let shader = wgpu_device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(labels.shader),
        source: wgpu::ShaderSource::Wgsl(DIRECT_ICON_SHADER.into()),
    });
    let uniform_bind_group_layout =
        wgpu_device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(labels.bind_group_layout),
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
    let atlas_bind_group_layout =
        wgpu_device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui_dom_layer_pool_icon_atlas_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
    let pipeline_layout = wgpu_device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(labels.pipeline_layout),
        bind_group_layouts: &[&uniform_bind_group_layout, &atlas_bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = wgpu_device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(labels.render_pipeline),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: &[gpu_icon_op_layout()],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let uniform_buffer = wgpu_device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(labels.uniform_buffer),
        size: u64::try_from(std::mem::size_of::<GpuIconPipelineUniform>())
            .expect("icon uniform size should fit u64"),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniform_bind_group = wgpu_device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(labels.bind_group),
        layout: &uniform_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    let atlas_sampler = wgpu_device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("ui_dom_layer_pool_icon_atlas_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    });
    IconRenderState {
        pipeline,
        gpu_buffer: create_instance_buffer(
            device,
            1,
            std::mem::size_of::<GpuIconOp>(),
            "ui_dom_layer_pool_icon_instances",
        ),
        gpu_capacity: 1,
        uniform_buffer,
        uniform_bind_group,
        atlas_bind_group_layout,
        atlas_sampler,
        atlas: None,
    }
}

fn install_ready_icon_atlas(
    device: &RenderDevice,
    queue: &RenderQueue,
    icon_state: &mut IconRenderState,
    pending_install: PendingIconAtlasInstall,
) {
    let texture_extent = Extent3d {
        width: pending_install.atlas_width_px,
        height: pending_install.atlas_height_px,
        depth_or_array_layers: 1,
    };
    let texture = device
        .wgpu_device()
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("ui_dom_layer_pool_icon_atlas_texture"),
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pending_install.alpha_mask_bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(pending_install.atlas_width_px),
            rows_per_image: Some(pending_install.atlas_height_px),
        },
        texture_extent,
    );
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let atlas_bind_group = device
        .wgpu_device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui_dom_layer_pool_icon_atlas_bind_group"),
            layout: &icon_state.atlas_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&icon_state.atlas_sampler),
                },
            ],
        });
    icon_state.atlas = Some(RenderOwnedIconAtlas {
        _texture: texture,
        _texture_view: texture_view,
        bind_group: atlas_bind_group,
        manifest: pending_install.validated_manifest,
    });
}

fn create_rect_pipeline_state(device: &RenderDevice) -> LayerPipelineState {
    create_pipeline_state(
        device,
        PipelineLabels {
            shader: "ui_dom_layer_pool_rect_shader",
            bind_group_layout: "ui_dom_layer_pool_rect_bind_group_layout",
            pipeline_layout: "ui_dom_layer_pool_rect_pipeline_layout",
            render_pipeline: "ui_dom_layer_pool_rect_pipeline",
            uniform_buffer: "ui_dom_layer_pool_rect_target_size",
            bind_group: "ui_dom_layer_pool_rect_bind_group",
            instance_buffer: "ui_dom_layer_pool_rect_instances",
            encoder: "ui_dom_layer_pool_rect_encoder",
            render_pass: "ui_dom_layer_pool_rect_pass",
        },
        DIRECT_RECT_SHADER,
        &[gpu_rect_op_layout()],
        std::mem::size_of::<RectPaintOp>(),
    )
}

fn create_debug_rect_pipeline_state(
    device: &RenderDevice,
    pipeline_name: &str,
    shader_source: &str,
) -> LayerPipelineState {
    let labels = pipeline_labels(pipeline_name);
    create_pipeline_state(
        device,
        labels,
        shader_source,
        &[gpu_debug_rect_op_layout()],
        std::mem::size_of::<GpuDebugRectOp>(),
    )
}

fn create_pipeline_state(
    device: &RenderDevice,
    labels: PipelineLabels,
    shader_source: &str,
    vertex_layouts: &[wgpu::VertexBufferLayout<'static>],
    instance_stride_bytes: usize,
) -> LayerPipelineState {
    let wgpu_device = device.wgpu_device();
    let shader = wgpu_device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(labels.shader),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    let bind_group_layout =
        wgpu_device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(labels.bind_group_layout),
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
    let pipeline_layout = wgpu_device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(labels.pipeline_layout),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = wgpu_device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(labels.render_pipeline),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: vertex_layouts,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let uniform_buffer = wgpu_device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(labels.uniform_buffer),
        size: u64::try_from(std::mem::size_of::<GpuRectPipelineUniform>())
            .expect("rect uniform size should fit into u64"),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = wgpu_device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(labels.bind_group),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    LayerPipelineState {
        pipeline,
        gpu_buffer: create_instance_buffer(
            device,
            1,
            instance_stride_bytes,
            labels.instance_buffer,
        ),
        gpu_capacity: 1,
        uniform_buffer,
        bind_group,
    }
}

fn gpu_rect_op_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: u64::try_from(std::mem::size_of::<RectPaintOp>())
            .expect("gpu rect instance stride should fit into buffer address"),
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &GPU_RECT_OP_ATTRIBUTES,
    }
}

fn gpu_debug_rect_op_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: u64::try_from(std::mem::size_of::<GpuDebugRectOp>())
            .expect("gpu debug rect instance stride should fit into buffer address"),
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &GPU_DEBUG_RECT_OP_ATTRIBUTES,
    }
}

fn gpu_icon_op_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: u64::try_from(std::mem::size_of::<GpuIconOp>())
            .expect("gpu icon instance stride should fit into buffer address"),
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &GPU_ICON_OP_ATTRIBUTES,
    }
}

fn gpu_text_run_op_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: u64::try_from(std::mem::size_of::<GpuTextRunOp>())
            .expect("gpu text run instance stride should fit into buffer address"),
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &GPU_TEXT_RUN_OP_ATTRIBUTES,
    }
}

fn ensure_gpu_capacity(
    device: &RenderDevice,
    pipeline: &mut LayerPipelineState,
    instance_count: usize,
    instance_stride_bytes: usize,
    instance_buffer_label: &'static str,
) -> bool {
    if instance_count <= pipeline.gpu_capacity {
        return false;
    }
    let required_capacity = instance_count.next_power_of_two();
    pipeline.gpu_buffer = create_instance_buffer(
        device,
        required_capacity,
        instance_stride_bytes,
        instance_buffer_label,
    );
    pipeline.gpu_capacity = required_capacity;
    true
}

fn ensure_icon_gpu_capacity(
    device: &RenderDevice,
    icon_state: &mut IconRenderState,
    instance_count: usize,
) -> bool {
    if instance_count <= icon_state.gpu_capacity {
        return false;
    }
    let required_capacity = instance_count.next_power_of_two();
    icon_state.gpu_buffer = create_instance_buffer(
        device,
        required_capacity,
        std::mem::size_of::<GpuIconOp>(),
        "ui_dom_layer_pool_icon_instances",
    );
    icon_state.gpu_capacity = required_capacity;
    true
}

fn create_instance_buffer(
    device: &RenderDevice,
    capacity: usize,
    instance_stride_bytes: usize,
    instance_buffer_label: &'static str,
) -> wgpu::Buffer {
    let buffer_size = u64::try_from(instance_stride_bytes * capacity)
        .expect("instance buffer size should fit into u64");
    device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: Some(instance_buffer_label),
        size: buffer_size.max(1),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pixel_density_scale(pixel_density: PixelDensity) -> f32 {
    f32::from(
        u16::try_from(pixel_density.multiplier())
            .expect("pixel density multiplier should fit into u16 for gpu scaling"),
    )
}

fn write_debug_rect_op(
    gpu_buffer: &wgpu::Buffer,
    queue: &RenderQueue,
    index: usize,
    op: GpuDebugRectOp,
) {
    let byte_offset = u64::try_from(index * std::mem::size_of::<GpuDebugRectOp>())
        .expect("debug rect write offset should fit into u64");
    queue.write_buffer(gpu_buffer, byte_offset, bytemuck::bytes_of(&op));
}

fn pipeline_labels(pipeline_name: &str) -> PipelineLabels {
    match pipeline_name {
        "text_run" => PipelineLabels {
            shader: "ui_dom_layer_pool_text_run_shader",
            bind_group_layout: "ui_dom_layer_pool_text_run_bind_group_layout",
            pipeline_layout: "ui_dom_layer_pool_text_run_pipeline_layout",
            render_pipeline: "ui_dom_layer_pool_text_run_pipeline",
            uniform_buffer: "ui_dom_layer_pool_text_run_target_size",
            bind_group: "ui_dom_layer_pool_text_run_bind_group",
            instance_buffer: "ui_dom_layer_pool_text_run_instances",
            encoder: "ui_dom_layer_pool_text_run_encoder",
            render_pass: "ui_dom_layer_pool_text_run_pass",
        },
        "svg" => PipelineLabels {
            shader: "ui_dom_layer_pool_svg_shader",
            bind_group_layout: "ui_dom_layer_pool_svg_bind_group_layout",
            pipeline_layout: "ui_dom_layer_pool_svg_pipeline_layout",
            render_pipeline: "ui_dom_layer_pool_svg_pipeline",
            uniform_buffer: "ui_dom_layer_pool_svg_target_size",
            bind_group: "ui_dom_layer_pool_svg_bind_group",
            instance_buffer: "ui_dom_layer_pool_svg_instances",
            encoder: "ui_dom_layer_pool_svg_encoder",
            render_pass: "ui_dom_layer_pool_svg_pass",
        },
        "image" => PipelineLabels {
            shader: "ui_dom_layer_pool_image_shader",
            bind_group_layout: "ui_dom_layer_pool_image_bind_group_layout",
            pipeline_layout: "ui_dom_layer_pool_image_pipeline_layout",
            render_pipeline: "ui_dom_layer_pool_image_pipeline",
            uniform_buffer: "ui_dom_layer_pool_image_target_size",
            bind_group: "ui_dom_layer_pool_image_bind_group",
            instance_buffer: "ui_dom_layer_pool_image_instances",
            encoder: "ui_dom_layer_pool_image_encoder",
            render_pass: "ui_dom_layer_pool_image_pass",
        },
        _ => panic!("unknown debug rect pipeline name"),
    }
}

fn icon_pipeline_labels() -> PipelineLabels {
    PipelineLabels {
        shader: "ui_dom_layer_pool_icon_shader",
        bind_group_layout: "ui_dom_layer_pool_icon_uniform_bind_group_layout",
        pipeline_layout: "ui_dom_layer_pool_icon_pipeline_layout",
        render_pipeline: "ui_dom_layer_pool_icon_pipeline",
        uniform_buffer: "ui_dom_layer_pool_icon_uniform_buffer",
        bind_group: "ui_dom_layer_pool_icon_uniform_bind_group",
        instance_buffer: "ui_dom_layer_pool_icon_instances",
        encoder: "ui_dom_layer_pool_icon_encoder",
        render_pass: "ui_dom_layer_pool_icon_pass",
    }
}

fn require_direct_gpu_executor(world: &World) -> Result<(), LayerPoolApplyError> {
    let Some(render_support) = world.get_resource::<UiDomRenderSupport>() else {
        return Err(LayerPoolApplyError::MissingDirectGpuExecutor);
    };
    if !render_support.direct_gpu_executor_available {
        return Err(LayerPoolApplyError::MissingDirectGpuExecutor);
    }
    Ok(())
}

fn enqueue_layer_invalidation(world: &mut World, lease: LayerLease) {
    world.init_resource::<LayerPoolRenderJobQueue>();
    world
        .resource_mut::<LayerPoolRenderJobQueue>()
        .push_invalidate(lease);
    world.init_resource::<RenderOpsHandlers>();
    for handler in &world.resource::<RenderOpsHandlers>().0 {
        handler.invalidate_lease(lease);
    }
}

fn enqueue_layer_invalidation_for_policy(
    world: &mut World,
    lease: LayerLease,
    policy: LayerPoolPolicy,
) {
    if policy == LayerPoolPolicy::External {
        assert!(
            world.contains_resource::<LayerPool>(),
            "ui-dom GPU layer work requires an installed LayerPool",
        );
        world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
            layer_pool
                .metrics
                .increment(LayerPoolMetricName::InvalidationBypassed);
        });
        return;
    }
    enqueue_layer_invalidation(world, lease);
}

fn increment_metric_count(
    metrics: &mut LayerPoolMetricStore,
    name: LayerPoolMetricName,
    count: u64,
) {
    for _ in 0..count {
        metrics.increment(name);
    }
}

fn flush_layer_pool_render_feedback(
    main_world: &mut MainWorld,
    feedback: &mut LayerPoolRenderFeedback,
) {
    if feedback.is_empty() {
        return;
    }
    assert!(
        main_world.contains_resource::<LayerPool>(),
        "ui-dom GPU layer work requires an installed LayerPool",
    );
    main_world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
        feedback.apply_to(&mut layer_pool);
    });
    feedback.clear();
}

fn layer_mapping_for_rect(layer_rect: LayerRect, pixel_density: PixelDensity) -> LayerMapping {
    let retained_rect = retained_rect_for_logical_rect(layer_rect, pixel_density);
    let rounded_left_px = retained_rect.origin_x.round();
    let rounded_top_px = retained_rect.origin_y.round();
    let rounded_right_px = (retained_rect.origin_x + retained_rect.width).round();
    let rounded_bottom_px = (retained_rect.origin_y + retained_rect.height).round();
    assert!(
        rounded_right_px >= rounded_left_px,
        "rounded retained x bounds should not invert the retained extent"
    );
    assert!(
        rounded_bottom_px >= rounded_top_px,
        "rounded retained y bounds should not invert the retained extent"
    );
    LayerMapping {
        image_width_px: (rounded_right_px - rounded_left_px).max(1.0),
        image_height_px: (rounded_bottom_px - rounded_top_px).max(1.0),
        layer_zero_x_in_image_px: -rounded_left_px,
        layer_zero_y_in_image_px: -rounded_top_px,
    }
}

fn texture_extent_px_from_layer_mapping(mapping: LayerMapping) -> (u32, u32) {
    (
        f32_to_u32(mapping.image_width_px),
        f32_to_u32(mapping.image_height_px),
    )
}

fn retained_rect_for_logical_rect(layer_rect: LayerRect, pixel_density: PixelDensity) -> LayerRect {
    let density_scale = pixel_density_scale(pixel_density);
    LayerRect {
        origin_x: layer_rect.origin_x * density_scale,
        origin_y: layer_rect.origin_y * density_scale,
        width: layer_rect.width * density_scale,
        height: layer_rect.height * density_scale,
    }
}

fn create_layer_surface_image(world: &mut World, image_size_px: (u32, u32)) -> Handle<Image> {
    if world.get_resource::<Assets<Image>>().is_none() {
        world.insert_resource(Assets::<Image>::default());
    }
    let mut image = Image::new_fill(
        Extent3d {
            width: image_size_px.0,
            height: image_size_px.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::STORAGE_BINDING
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    world.resource_mut::<Assets<Image>>().add(image)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::layer_pool_text_render as text_render;
    use super::*;
    use bevy::{
        prelude::{Entity, World},
        render::sync_world::MainEntity,
    };
    use collections::lru_cache::LruCache;
    use cosmic_text::CacheKeyFlags;
    use cosmic_text::Family as CosmicFamily;
    use cosmic_text::fontdb::{ID as FontDbId, Weight as FontWeight};
    use slotmap::SlotMap;
    use std::sync::Arc;

    fn record_fields(pool: &LayerPool, lease: LayerLease) -> (u32, u32, f32, f32) {
        let record = pool
            .layers
            .get(lease.layer_id)
            .expect("lease should resolve to a layer record");
        (
            record.image_width_px,
            record.image_height_px,
            record.layer_zero_x_in_image_px,
            record.layer_zero_y_in_image_px,
        )
    }

    fn metric_count(pool: &LayerPool, name: LayerPoolMetricName) -> u64 {
        pool.count(name)
    }

    fn assert_metric_counts(pool: &LayerPool, expected_counts: &[(LayerPoolMetricName, u64)]) {
        for name in super::layer_pool_metric_name::LAYER_POOL_METRIC_NAMES {
            let expected = expected_counts
                .iter()
                .find_map(|(expected_name, expected_count)| {
                    if *expected_name == name {
                        Some(*expected_count)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            assert_eq!(
                metric_count(pool, name),
                expected,
                "unexpected count for {name:?}"
            );
        }
    }

    fn assert_history_names(pool: &LayerPool, expected_names: &[LayerPoolMetricName]) {
        let actual_names: Vec<_> = pool.history().map(|entry| entry.name).collect();
        let mut search_from = 0;
        for &expected in expected_names {
            let pos = actual_names[search_from..]
                .iter()
                .position(|n| *n == expected)
                .unwrap_or_else(|| {
                    panic!("{expected:?} should be present in history after position {search_from}")
                });
            search_from += pos + 1;
        }
    }

    fn single_rect_plan() -> LayerPaintPlan {
        LayerPaintPlan {
            rects: vec![RectPaintOp {
                position_px: [1.0, 2.0],
                size_px: [8.0, 6.0],
                fill_rgba: [0.2, 0.4, 0.6, 1.0],
                corner_radii_px: [0.0, 0.0, 0.0, 0.0],
                border_widths_px: [0.0, 0.0, 0.0, 0.0],
                border_top_rgba: [0.0, 0.0, 0.0, 0.0],
                border_right_rgba: [0.0, 0.0, 0.0, 0.0],
                border_bottom_rgba: [0.0, 0.0, 0.0, 0.0],
                border_left_rgba: [0.0, 0.0, 0.0, 0.0],
            }],
            ..LayerPaintPlan::default()
        }
    }

    fn clear_pending_jobs(world: &mut World) {
        world.init_resource::<LayerPoolRenderJobQueue>();
        let mut pending_jobs = world.resource_mut::<LayerPoolRenderJobQueue>();
        pending_jobs.jobs.clear();
    }

    fn install_layer_pool(world: &mut World, pixel_density: PixelDensity) {
        world.insert_resource(LayerPool::new(LayerConfig::new(pixel_density)));
    }

    fn install_layer_pool_render_executor(world: &mut World, pixel_density: PixelDensity) {
        world.insert_resource(LayerPoolRenderExecutor::new(LayerConfig::new(
            pixel_density,
        )));
    }

    fn sample_text_run_op() -> TextRunPaintOp {
        let mut text_run_ids = SlotMap::with_key();
        TextRunPaintOp {
            text_run_id: text_run_ids.insert(()),
            position_px: [4.0, 6.0],
            size_px: [120.0, 40.0],
            font_size_px: 16.0,
            line_height_px: 20.0,
            wrap_mode: super::TextWrapMode::WordOrGlyph,
            alignment: super::TextAlign::Left,
            color_rgba: [1.0, 1.0, 1.0, 1.0],
        }
    }

    fn sample_run_record() -> super::RenderTextRunRecord {
        super::RenderTextRunRecord {
            text: Arc::<str>::from("Density"),
            language_hint_id: TextLanguageId::English,
            font_family_id: super::TextFontFamilyId::UiSans,
        }
    }

    fn sample_text_buffer_key(text_run_index: usize) -> text_render::TextBufferKey {
        let mut text_run = sample_text_run_op();
        let mut text_run_ids = SlotMap::with_key();
        let mut text_run_id = text_run_ids.insert(());
        for _ in 1..text_run_index {
            text_run_id = text_run_ids.insert(());
        }
        text_run.text_run_id = text_run_id;
        text_render::text_buffer_key(&text_run)
    }

    fn text_run_at_intrinsic_width(text: &str) -> (TextRunPaintOp, super::RenderTextRunRecord) {
        let intrinsic = text_layout::with_repo_text_layout_context(|context| {
            context.measure_intrinsic(&text_layout::TextLayoutRequest::for_text(text, 16.0))
        });
        let mut text_run = sample_text_run_op();
        text_run.size_px = [intrinsic.max_content_width_px, intrinsic.line_height_px];
        text_run.line_height_px = intrinsic.line_height_px;
        (
            text_run,
            super::RenderTextRunRecord {
                text: Arc::<str>::from(text),
                language_hint_id: TextLanguageId::from_text_hint(text),
                font_family_id: super::TextFontFamilyId::UiSans,
            },
        )
    }

    fn sample_glyph_mask_key(font_size_px: f32) -> text_render::GlyphMaskKey {
        let (cache_key, _, _) = CacheKey::new(
            FontDbId::default(),
            7,
            font_size_px,
            (0.0, 0.0),
            FontWeight(400),
            CacheKeyFlags::empty(),
        );
        text_render::GlyphMaskKey { cache_key }
    }

    fn sample_icon_manifest() -> ValidatedUiIconAtlasManifest {
        let sample_entry = ValidatedUiIconAtlasEntry {
            packed_rect_px: [10, 20, 24, 24],
        };
        let entries =
            std::iter::repeat_n(sample_entry, ui_icons::UiIconId::ALL.len()).collect::<Vec<_>>();
        ValidatedUiIconAtlasManifest {
            atlas_width_px: 64,
            atlas_height_px: 64,
            design_box_size_px: 24,
            lookup: ValidatedUiIconAtlasLookup::from_entries_for_tests(entries),
        }
    }

    #[test]
    fn destroying_a_layer_lease_removes_the_retained_record() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);

        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.25, -0.25, 120.0, 40.0));
        let destroyed = LayerPool::destroy_layer_lease_in_world(&mut world, lease);

        assert!(destroyed.is_ok());
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert!(
                layer_pool.layers.is_empty(),
                "destroyed leases should remove the slotmap record"
            );
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::LayerDestroyed, 1),
                ],
            );
            let history_names: Vec<_> = layer_pool.history().map(|entry| entry.name).collect();
            let created_pos = history_names
                .iter()
                .position(|n| *n == LayerPoolMetricName::LayerCreated)
                .expect("LayerCreated should be present in history");
            let destroyed_pos = history_names
                .iter()
                .position(|n| *n == LayerPoolMetricName::LayerDestroyed)
                .expect("LayerDestroyed should be present in history");
            assert!(
                created_pos < destroyed_pos,
                "LayerCreated must appear before LayerDestroyed"
            );
        });
    }

    #[test]
    fn layer_pool_metric_count_matches_the_snapshot_read_api() {
        let mut layer_pool = LayerPool::new(LayerConfig::default());
        let lease = layer_pool.create_layer(super::LayerRect::from_tuple((0.0, 0.0, 10.0, 10.0)));
        assert!(layer_pool.destroy_layer(lease).is_ok());

        assert_eq!(
            layer_pool.count(LayerPoolMetricName::LayerCreated),
            layer_pool
                .metrics()
                .count(LayerPoolMetricName::LayerCreated)
        );
        assert_eq!(
            layer_pool.count(LayerPoolMetricName::LayerDestroyed),
            layer_pool
                .metrics()
                .count(LayerPoolMetricName::LayerDestroyed)
        );
    }

    #[test]
    fn rect_paint_ops_are_uploaded_directly_as_the_gpu_contract() {
        let rects = [RectPaintOp {
            position_px: [21.0, 16.0],
            size_px: [13.0, 9.0],
            fill_rgba: [0.2, 0.4, 0.6, 0.8],
            corner_radii_px: [6.0, 5.0, 4.0, 3.0],
            border_widths_px: [2.0, 4.0, 6.0, 8.0],
            border_top_rgba: [1.0, 0.0, 0.0, 1.0],
            border_right_rgba: [0.0, 1.0, 0.0, 1.0],
            border_bottom_rgba: [0.0, 0.0, 1.0, 1.0],
            border_left_rgba: [1.0, 1.0, 0.0, 1.0],
        }];

        let raw_bytes: &[u8] = bytemuck::cast_slice(&rects);

        assert_eq!(raw_bytes.len(), std::mem::size_of::<RectPaintOp>());
        assert_eq!(
            u64::try_from(std::mem::size_of::<RectPaintOp>())
                .expect("rect op stride should fit into u64"),
            super::gpu_rect_op_layout().array_stride
        );
    }

    #[test]
    fn rect_shader_parses_as_wgsl() {
        let module = naga::front::wgsl::parse_str(super::DIRECT_RECT_SHADER)
            .expect("rect layer shader should parse as WGSL");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("rect layer shader should validate as WGSL");
    }

    #[test]
    fn icon_uv_window_maps_to_the_packed_square_rect() {
        let manifest = sample_icon_manifest();
        let entry = manifest.lookup.entry(ui_icons::UiIconId::Scene);

        let (uv_min, uv_max) = icon_uv_window(entry, &manifest);

        assert_eq!(uv_min, [10.0 / 64.0, 20.0 / 64.0]);
        assert_eq!(uv_max, [34.0 / 64.0, 44.0 / 64.0]);
    }

    #[test]
    fn gpu_icon_op_preserves_authored_position_and_size_exactly() {
        let manifest = sample_icon_manifest();
        let icon = IconPaintOp {
            position_px: [19.0, 23.0],
            size_px: [17.0, 21.0],
            icon_id: ui_icons::UiIconId::Scene,
            color_rgba: [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 40.0 / 255.0],
        };

        let gpu_op = super::gpu_icon_op(&icon, &manifest);

        assert_eq!(gpu_op.position_px, icon.position_px);
        assert_eq!(gpu_op.size_px, icon.size_px);
    }

    #[test]
    fn stale_layer_leases_are_rejected_before_mutation() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 10.0, 10.0));
        let stale = LayerLease {
            layer_id: lease.layer_id,
            generation: lease
                .generation
                .checked_add(1)
                .expect("test stale generation should remain in bounds"),
        };

        let updated =
            LayerPool::update_layer_lease_in_world(&mut world, stale, (5.0, 5.0, 20.0, 20.0));

        assert_eq!(
            updated.expect_err("stale generation should be rejected"),
            "layer lease generation should match the current slot generation"
        );
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::StaleLeaseRejected, 1),
                ],
            );
        });
    }

    #[test]
    fn same_float_rect_maps_identically_on_repeated_updates() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.49, -0.49, 120.2, 31.2));
        let before = world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            record_fields(&layer_pool, lease)
        });

        assert!(
            LayerPool::update_layer_lease_in_world(&mut world, lease, (0.49, -0.49, 120.2, 31.2),)
                .is_ok()
        );

        let after = world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            record_fields(&layer_pool, lease)
        });
        assert_eq!(before, after);
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(&layer_pool, &[(LayerPoolMetricName::LayerCreated, 1)]);
        });
    }

    #[test]
    fn exact_mapping_derives_from_rounded_edges_only() {
        let mapping = super::layer_mapping_for_rect(
            super::LayerRect::from_tuple((0.49, 0.49, 0.51, 0.51)),
            PixelDensity::X1,
        );

        assert!((mapping.image_width_px - 1.0).abs() <= f32::EPSILON);
        assert!((mapping.image_height_px - 1.0).abs() <= f32::EPSILON);
        assert!(mapping.layer_zero_x_in_image_px.abs() <= f32::EPSILON);
        assert!(mapping.layer_zero_y_in_image_px.abs() <= f32::EPSILON);
    }

    #[test]
    fn logical_retained_bounds_scale_to_exact_snapped_image_sizes_at_supported_densities() {
        let logical_rect = super::LayerRect::from_tuple((0.0, 0.0, 32.0, 16.0));

        let x1 = super::layer_mapping_for_rect(logical_rect, PixelDensity::X1);
        let x2 = super::layer_mapping_for_rect(logical_rect, PixelDensity::X2);
        let x4 = super::layer_mapping_for_rect(logical_rect, PixelDensity::X4);

        assert_eq!(super::texture_extent_px_from_layer_mapping(x1), (32, 16));
        assert_eq!(super::texture_extent_px_from_layer_mapping(x2), (64, 32));
        assert_eq!(super::texture_extent_px_from_layer_mapping(x4), (128, 64));
    }

    #[test]
    fn fractional_retained_origin_stays_tucked_into_the_snapped_top_left_across_densities() {
        let logical_rect = super::LayerRect::from_tuple((-0.25, -0.25, 32.0, 16.0));

        let x1 = super::layer_mapping_for_rect(logical_rect, PixelDensity::X1);
        let x2 = super::layer_mapping_for_rect(logical_rect, PixelDensity::X2);
        let x4 = super::layer_mapping_for_rect(logical_rect, PixelDensity::X4);

        assert_eq!(super::texture_extent_px_from_layer_mapping(x1), (32, 16));
        assert_eq!(super::texture_extent_px_from_layer_mapping(x2), (65, 33));
        assert_eq!(super::texture_extent_px_from_layer_mapping(x4), (128, 64));
        assert_eq!(
            (x1.layer_zero_x_in_image_px, x1.layer_zero_y_in_image_px),
            (0.0, 0.0)
        );
        assert_eq!(
            (x2.layer_zero_x_in_image_px, x2.layer_zero_y_in_image_px),
            (1.0, 1.0)
        );
        assert_eq!(
            (x4.layer_zero_x_in_image_px, x4.layer_zero_y_in_image_px),
            (1.0, 1.0)
        );
    }

    #[test]
    fn glyph_mask_texture_extent_rejects_zero_dimensions() {
        assert!(text_render::glyph_mask_texture_extent(0, 12).is_none());
        assert!(text_render::glyph_mask_texture_extent(12, 0).is_none());
        assert_eq!(
            text_render::glyph_mask_texture_extent(12, 8)
                .expect("non-zero glyph extents should stay uploadable")
                .depth_or_array_layers,
            1
        );
    }

    #[test]
    fn text_run_preload_queue_covers_each_language_with_unique_glyph_requests() {
        let preload_warmup_queue = text_render::create_text_run_preload_queue(PixelDensity::X1);
        let expected_requests_per_language = 100;

        for language_hint_id in [
            TextLanguageId::English,
            TextLanguageId::Japanese,
            TextLanguageId::SimplifiedChinese,
            TextLanguageId::Korean,
            TextLanguageId::Thai,
        ] {
            assert_eq!(
                preload_warmup_queue
                    .requests
                    .iter()
                    .filter(|request| request.key.language_hint_id == language_hint_id)
                    .count(),
                expected_requests_per_language,
                "each language should enqueue about one hundred preload warmups",
            );
        }
        assert_eq!(
            preload_warmup_queue.requests.len(),
            preload_warmup_queue.unique_request_count(),
            "preload descriptors should remain unique after dedupe",
        );
        assert!(
            preload_warmup_queue.requests.iter().all(|request| {
                request.key.warmup_kind == text_render::GenericWarmupKind::GlyphMask
                    && request.key.raster_scale_bits
                        == pixel_density_scale(PixelDensity::X1).to_bits()
            }),
            "startup warmups should stay on glyph-mask descriptors at the declared raster scale",
        );
    }

    #[test]
    fn preload_queue_uses_the_configured_density_scale() {
        let preload_warmup_queue = text_render::create_text_run_preload_queue(PixelDensity::X4);

        assert!(preload_warmup_queue.requests.iter().all(|request| {
            request.key.raster_scale_bits == pixel_density_scale(PixelDensity::X4).to_bits()
        }));
    }

    #[test]
    fn installed_layer_pool_keeps_the_chosen_density() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X4);

        assert_eq!(
            world.resource::<LayerPool>().pixel_density(),
            PixelDensity::X4
        );
    }

    #[test]
    fn layer_pool_world_facade_requires_an_installed_layer_pool() {
        let mut world = World::new();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 1.0, 1.0));
        }));

        assert!(
            result.is_err(),
            "layer pool calls should fail without LayerPool"
        );
    }

    #[test]
    fn resolve_text_run_id_reuses_matching_signatures() {
        let mut layer_pool = LayerPool::new(LayerConfig::default());
        let text_run_signature = TextRunSignature::from_text(Arc::<str>::from("Window Control"));

        let first = layer_pool.resolve_text_run_id_from_hint(&text_run_signature, None);
        let second = layer_pool.resolve_text_run_id_from_hint(&text_run_signature, None);

        assert_eq!(first, second);
        assert_eq!(layer_pool.drain_pending_text_run_changes().len(), 1);
        assert!(layer_pool.drain_pending_text_run_removals().is_empty());
    }

    #[test]
    fn resolve_text_run_id_ignores_stale_hints() {
        let mut layer_pool = LayerPool::new(LayerConfig::default());
        let window_control = TextRunSignature::from_text(Arc::<str>::from("Window Control"));
        let texture_plane = TextRunSignature::from_text(Arc::<str>::from("Window Texture Plane"));

        let stale_hint = layer_pool.resolve_text_run_id_from_hint(&window_control, None);
        let refreshed = layer_pool.resolve_text_run_id_from_hint(&texture_plane, Some(stale_hint));
        let reused = layer_pool.resolve_text_run_id_from_hint(&texture_plane, Some(refreshed));

        assert_ne!(stale_hint, refreshed);
        assert_eq!(refreshed, reused);
    }

    #[test]
    fn text_buffer_key_uses_only_density_free_text_fields() {
        let text_run = sample_text_run_op();
        assert_eq!(
            text_render::text_buffer_key(&text_run),
            text_render::TextBufferKey {
                text_run_id: text_run.text_run_id,
                font_size_bits: text_run.font_size_px.to_bits(),
                line_height_bits: text_run.line_height_px.to_bits(),
                wrap_mode: text_run.wrap_mode,
                alignment: text_run.alignment,
                width_bits: text_run.size_px[0].to_bits(),
                height_bits: text_run.size_px[1].to_bits(),
            }
        );
    }

    #[test]
    fn text_buffer_lru_evicts_oldest_density_free_keys_under_density_aware_rendering() {
        let mut cache = LruCache::<text_render::TextBufferKey, u8, 2>::default();
        let first = sample_text_buffer_key(1);
        let second = sample_text_buffer_key(2);
        let third = sample_text_buffer_key(3);

        let _ = cache.insert(&first, 1);
        let _ = cache.insert(&second, 2);
        let _ = cache.insert(&third, 3);

        assert!(!cache.contains(&first));
        assert!(cache.contains(&second));
        assert!(cache.contains(&third));
    }

    #[test]
    fn glyph_mask_lru_evicts_oldest_density_specific_keys() {
        let mut cache = LruCache::<text_render::GlyphMaskKey, u8, 2>::default();
        let x1 = sample_glyph_mask_key(16.0);
        let x2 = sample_glyph_mask_key(32.0);
        let x4 = sample_glyph_mask_key(64.0);

        let _ = cache.insert(&x1, 1);
        let _ = cache.insert(&x2, 2);
        let _ = cache.insert(&x4, 3);

        assert!(!cache.contains(&x1));
        assert!(cache.contains(&x2));
        assert!(cache.contains(&x4));
    }

    #[test]
    fn glyph_cache_identity_changes_when_density_changes_but_layout_anchor_stays_stable() {
        let text_run = sample_text_run_op();
        let run_record = sample_run_record();
        let mut font_system = text_render::create_text_run_font_system();
        let cached_buffer =
            text_render::build_text_buffer(&mut font_system, &run_record, &text_run);
        let layout_run = cached_buffer
            .buffer
            .layout_runs()
            .next()
            .expect("sample text should produce a layout run");
        let glyph = layout_run
            .glyphs
            .first()
            .cloned()
            .expect("sample text should produce a glyph");
        let baseline_y = text_run.position_px[1] + layout_run.line_y;
        let x1 =
            text_render::glyph_in_retained_pixels(&glyph, &text_run, baseline_y, PixelDensity::X1);
        let x2 =
            text_render::glyph_in_retained_pixels(&glyph, &text_run, baseline_y, PixelDensity::X2);

        assert!((baseline_y - (text_run.position_px[1] + layout_run.line_y)).abs() <= f32::EPSILON);
        assert_ne!(x1.cache_key, x2.cache_key);
        assert!(
            ((i32_to_f32(x1.x) / pixel_density_scale(PixelDensity::X1))
                - (i32_to_f32(x2.x) / pixel_density_scale(PixelDensity::X2)))
            .abs()
                <= 0.5,
            "density changes should preserve the same authored-space x anchor",
        );
        assert!(
            ((i32_to_f32(x1.y) / pixel_density_scale(PixelDensity::X1))
                - (i32_to_f32(x2.y) / pixel_density_scale(PixelDensity::X2)))
            .abs()
                <= 0.5,
            "density changes should preserve the same authored-space baseline anchor",
        );
    }

    #[test]
    fn render_buffer_keeps_rtmp_server_single_line_at_intrinsic_width() {
        let (text_run, run_record) = text_run_at_intrinsic_width("RTMP Server");
        let mut font_system = text_render::create_text_run_font_system();
        let cached_buffer =
            text_render::build_text_buffer(&mut font_system, &run_record, &text_run);

        assert_eq!(cached_buffer.buffer.layout_runs().count(), 1);
    }

    #[test]
    fn render_buffer_keeps_session_state_single_line_at_intrinsic_width() {
        let (text_run, run_record) = text_run_at_intrinsic_width("Session State");
        let mut font_system = text_render::create_text_run_font_system();
        let cached_buffer =
            text_render::build_text_buffer(&mut font_system, &run_record, &text_run);

        assert_eq!(cached_buffer.buffer.layout_runs().count(), 1);
    }

    #[test]
    fn render_buffer_builds_layout_glyphs_for_input_like_single_line_runs() {
        let mut text_run = sample_text_run_op();
        text_run.size_px = [104.0, 24.0];
        text_run.wrap_mode = TextWrapMode::None;
        let run_record = super::RenderTextRunRecord {
            text: Arc::<str>::from("hello"),
            language_hint_id: TextLanguageId::English,
            font_family_id: super::TextFontFamilyId::UiSans,
        };
        let mut font_system = text_render::create_text_run_font_system();
        let cached_buffer =
            text_render::build_text_buffer(&mut font_system, &run_record, &text_run);
        let layout_run = cached_buffer
            .buffer
            .layout_runs()
            .next()
            .expect("input-like single-line run should produce one layout run");

        assert!(
            !layout_run.glyphs.is_empty(),
            "input-like single-line run should produce drawable glyphs in the retained text buffer",
        );
    }

    #[test]
    fn text_run_repo_font_preference_uses_language_specific_repo_families() {
        assert_eq!(
            text_layout::text_render::preferred_repo_font_family(
                TextFontFamilyId::UiSans,
                TextLanguageId::English,
            ),
            CosmicFamily::Name(text_layout::text_render::repo_default_sans_serif_family_name())
        );
        assert_eq!(
            text_layout::text_render::preferred_repo_font_family(
                TextFontFamilyId::UiSans,
                TextLanguageId::Japanese,
            ),
            CosmicFamily::Name("Source Han Sans")
        );
        assert_eq!(
            text_layout::text_render::preferred_repo_font_family(
                TextFontFamilyId::UiSans,
                TextLanguageId::SimplifiedChinese,
            ),
            CosmicFamily::Name("Source Han Sans SC")
        );
        assert_eq!(
            text_layout::text_render::preferred_repo_font_family(
                TextFontFamilyId::UiSans,
                TextLanguageId::Korean,
            ),
            CosmicFamily::Name("Source Han Sans K")
        );
    }

    #[test]
    fn missing_text_glyph_requires_debug_rect_fallback() {
        assert_eq!(
            text_render::glyph_requires_debug_fallback(0),
            text_render::TextGlyphFallbackDecision::DebugRect
        );
        assert_eq!(
            text_render::glyph_requires_debug_fallback(42),
            text_render::TextGlyphFallbackDecision::Continue
        );
    }

    #[test]
    fn glyph_mask_probe_promotes_visible_non_mask_glyphs_to_debug_fallback() {
        assert_eq!(
            text_render::glyph_mask_missing_mask_result(true),
            text_render::GlyphMaskMissingMaskResult::Unsupported
        );
    }

    #[test]
    fn glyph_mask_probe_keeps_invisible_glyphs_as_non_fallback_skips() {
        assert_eq!(
            text_render::glyph_mask_missing_mask_result(false),
            text_render::GlyphMaskMissingMaskResult::Skip
        );
    }

    #[test]
    fn retained_surface_lifecycle_reuses_matching_surfaces_and_tracks_order() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let first = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 65.0, 10.0));
        assert!(LayerPool::acquire_layer_surface_in_world(&mut world, first).is_ok());
        let reused_handle_id =
            world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
                layer_pool
                    .layers
                    .get(first.layer_id)
                    .and_then(|record| record.image.as_ref())
                    .expect("first acquire should bind a surface")
                    .id()
            });
        assert!(LayerPool::destroy_layer_lease_in_world(&mut world, first).is_ok());

        let second = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 65.0, 10.0));
        assert!(LayerPool::acquire_layer_surface_in_world(&mut world, second).is_ok());

        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_eq!(
                layer_pool
                    .layers
                    .get(second.layer_id)
                    .and_then(|record| record.image.as_ref())
                    .expect("second acquire should bind a surface")
                    .id(),
                reused_handle_id
            );
            assert_history_names(
                &layer_pool,
                &[
                    LayerPoolMetricName::LayerCreated,
                    LayerPoolMetricName::SurfaceAllocated,
                    LayerPoolMetricName::SurfaceReleased,
                    LayerPoolMetricName::LayerDestroyed,
                    LayerPoolMetricName::LayerCreated,
                    LayerPoolMetricName::SurfaceReused,
                ],
            );
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 2),
                    (LayerPoolMetricName::SurfaceAllocated, 1),
                    (LayerPoolMetricName::SurfaceReused, 1),
                    (LayerPoolMetricName::SurfaceReleased, 1),
                    (LayerPoolMetricName::LayerDestroyed, 1),
                ],
            );
        });
    }

    #[test]
    fn resizing_a_layer_only_releases_the_surface_when_growth_requires_it() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let growing_lease =
            LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 65.0, 10.0));
        assert!(LayerPool::acquire_layer_surface_in_world(&mut world, growing_lease).is_ok());
        assert!(
            LayerPool::update_layer_lease_in_world(
                &mut world,
                growing_lease,
                (0.0, 0.0, 300.0, 10.0)
            )
            .is_ok()
        );
        let shrinking_lease =
            LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 513.0, 10.0));
        assert!(
            LayerPool::update_layer_lease_in_world(
                &mut world,
                shrinking_lease,
                (0.0, 0.0, 65.0, 10.0)
            )
            .is_ok()
        );

        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            let grown = layer_pool
                .layers
                .get(growing_lease.layer_id)
                .expect("grown lease should resolve");
            let shrunk = layer_pool
                .layers
                .get(shrinking_lease.layer_id)
                .expect("shrunk lease should resolve");
            assert!(grown.image.is_none(), "growth should drop the old binding");
            assert_eq!((grown.image_width_px, grown.image_height_px), (300, 10));
            assert_eq!((shrunk.image_width_px, shrunk.image_height_px), (513, 10));
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 2),
                    (LayerPoolMetricName::SurfaceAllocated, 1),
                    (LayerPoolMetricName::LayerMappingUpdated, 2),
                    (LayerPoolMetricName::SurfaceGrew, 1),
                    (LayerPoolMetricName::SurfaceReleased, 1),
                ],
            );
        });
    }

    #[test]
    fn stale_layer_mutations_preserve_the_current_surface_binding() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 65.0, 10.0));
        assert!(LayerPool::acquire_layer_surface_in_world(&mut world, lease).is_ok());
        let current_handle_id =
            world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
                layer_pool
                    .layers
                    .get(lease.layer_id)
                    .and_then(|record| record.image.as_ref())
                    .expect("lease should have a current surface")
                    .id()
            });
        let stale = LayerLease {
            layer_id: lease.layer_id,
            generation: lease
                .generation
                .checked_add(1)
                .expect("stale generation should remain within bounds"),
        };

        let update_result =
            LayerPool::update_layer_lease_in_world(&mut world, stale, (0.0, 0.0, 300.0, 10.0));
        let release_result = LayerPool::release_layer_surface_in_world(&mut world, stale);

        assert!(update_result.is_err());
        assert!(release_result.is_err());
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            let current_record = layer_pool
                .layers
                .get(lease.layer_id)
                .expect("current lease should still resolve");
            let current_image_id = current_record
                .image
                .as_ref()
                .expect("current surface should still be bound")
                .id();
            assert_eq!(current_image_id, current_handle_id);
            assert_eq!(current_record.image_width_px, 65);
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::SurfaceAllocated, 1),
                    (LayerPoolMetricName::StaleLeaseRejected, 2),
                ],
            );
        });
    }

    #[test]
    fn non_empty_plan_with_direct_gpu_executor_acquires_target_and_stages_apply_without_repaint_metrics()
     {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        let handler = RenderOpsHandler::new();

        let target = LayerPool::acquire_paint_target_in_world(&mut world, lease)
            .expect("direct gpu executor should accept an acquired retained target")
            .expect("normal policy should not suppress uploads");
        let policy = target.policy;
        let _ = handler.stage(
            StagedLayerApply {
                lease,
                image_handle: target.image_handle,
                layer_zero_in_image_px: target.layer_zero_in_image_px,
                plan: single_rect_plan(),
            },
            policy,
        );

        handler.with_staged_applies(|applies| {
            assert_eq!(
                applies.len(),
                1,
                "staged apply should remain pending until the render world drains it"
            );
        });
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::SurfaceAllocated, 1),
                ],
            );
            assert_history_names(
                &layer_pool,
                &[
                    LayerPoolMetricName::LayerCreated,
                    LayerPoolMetricName::SurfaceAllocated,
                ],
            );
        });
    }

    #[test]
    fn staged_deferred_best_effort_apply_promotes_for_the_next_cycle() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let first = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        let second = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        let handler = RenderOpsHandler::new();
        let first_target = LayerPool::acquire_paint_target_in_world(&mut world, first)
            .expect("first target should acquire")
            .expect("best-effort policy should not suppress");
        let second_target = LayerPool::acquire_paint_target_in_world(&mut world, second)
            .expect("second target should acquire")
            .expect("best-effort policy should not suppress");

        let _ = handler.stage(
            StagedLayerApply {
                lease: first,
                image_handle: first_target.image_handle,
                layer_zero_in_image_px: first_target.layer_zero_in_image_px,
                plan: single_rect_plan(),
            },
            LayerPoolPolicy::BestEffort,
        );
        let _ = handler.stage(
            StagedLayerApply {
                lease: second,
                image_handle: second_target.image_handle,
                layer_zero_in_image_px: second_target.layer_zero_in_image_px,
                plan: single_rect_plan(),
            },
            LayerPoolPolicy::BestEffort,
        );

        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());
        let first_batch = handler.take_staged();
        for apply in first_batch.applies {
            executor.stage_apply(apply);
        }
        assert_eq!(executor.pending_applies.len(), 1);
        let second_batch = handler.take_staged();
        for apply in second_batch.applies {
            executor.stage_apply(apply);
        }
        assert_eq!(executor.pending_applies.len(), 2);
    }

    #[test]
    fn installed_layer_pool_render_executor_keeps_the_chosen_density() {
        let mut world = World::new();
        install_layer_pool_render_executor(&mut world, PixelDensity::X4);

        assert_eq!(
            world
                .resource::<LayerPoolRenderExecutor>()
                .layer_config
                .pixel_density(),
            PixelDensity::X4
        );
    }

    #[test]
    fn root_surface_order_jobs_retain_multiple_attached_roots() {
        let first_root_parent: MainEntity = Entity::from_bits(1).into();
        let second_root_parent: MainEntity = Entity::from_bits(2).into();
        let first_surface_a: MainEntity = Entity::from_bits(10).into();
        let first_surface_b: MainEntity = Entity::from_bits(11).into();
        let second_surface: MainEntity = Entity::from_bits(20).into();
        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());

        executor.apply_job(LayerPoolRenderJob::ReplaceRootSurfaceOrder {
            attached_root_parent: first_root_parent,
            parent_z_order: 4.0,
            ordered_surface_entities: vec![first_surface_a, first_surface_b],
        });
        executor.apply_job(LayerPoolRenderJob::ReplaceRootSurfaceOrder {
            attached_root_parent: second_root_parent,
            parent_z_order: 1.0,
            ordered_surface_entities: vec![second_surface],
        });
        executor.apply_job(LayerPoolRenderJob::DetachRootSurfaceOrder {
            attached_root_parent: first_root_parent,
        });

        assert_eq!(
            executor.ordered_surface_entities_for_attached_root_parent(first_root_parent),
            None,
        );
        assert_eq!(
            executor.ordered_surface_entities_for_attached_root_parent(second_root_parent),
            Some([second_surface].as_slice()),
        );
        assert_eq!(
            executor.parent_z_order_for_attached_root_parent(second_root_parent),
            Some(1.0),
        );
        assert_eq!(
            executor.attached_root_parent_for_surface_entity(second_surface),
            Some(second_root_parent),
        );
        assert_eq!(
            executor.attached_root_parent_for_surface_entity(first_surface_a),
            None,
        );
    }

    #[test]
    fn root_surface_order_queue_replaces_previous_job_for_the_same_parent() {
        let root_parent: MainEntity = Entity::from_bits(1).into();
        let first_surface: MainEntity = Entity::from_bits(10).into();
        let second_surface: MainEntity = Entity::from_bits(11).into();
        let mut pending_jobs = LayerPoolRenderJobQueue::default();

        pending_jobs.replace_root_surface_order(root_parent, 3.0, vec![first_surface]);
        pending_jobs.replace_root_surface_order(root_parent, 4.0, vec![second_surface]);

        assert_eq!(pending_jobs.jobs.len(), 1);
        assert!(matches!(
            pending_jobs.jobs.first(),
            Some(LayerPoolRenderJob::ReplaceRootSurfaceOrder {
                attached_root_parent,
                parent_z_order,
                ordered_surface_entities,
            }) if *attached_root_parent == root_parent
                && parent_z_order.to_bits() == 4.0_f32.to_bits()
                && ordered_surface_entities.as_slice() == [second_surface].as_slice()
        ));
    }

    #[test]
    #[should_panic(
        expected = "surface order updates should not reuse one surface entity across roots"
    )]
    fn root_surface_order_rejects_cross_root_surface_reuse() {
        let first_root_parent: MainEntity = Entity::from_bits(1).into();
        let second_root_parent: MainEntity = Entity::from_bits(2).into();
        let shared_surface: MainEntity = Entity::from_bits(10).into();
        let mut executor = LayerPoolRenderExecutor::new(LayerConfig::default());

        executor.apply_job(LayerPoolRenderJob::ReplaceRootSurfaceOrder {
            attached_root_parent: first_root_parent,
            parent_z_order: 2.0,
            ordered_surface_entities: vec![shared_surface],
        });
        executor.apply_job(LayerPoolRenderJob::ReplaceRootSurfaceOrder {
            attached_root_parent: second_root_parent,
            parent_z_order: 2.0,
            ordered_surface_entities: vec![shared_surface],
        });
    }

    #[test]
    fn offscreen_policy_suppresses_automatic_retained_updates() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        LayerPool::set_layer_policy_in_world(&mut world, lease, LayerPoolPolicy::Offscreen)
            .expect("offscreen policy should update");
        clear_pending_jobs(&mut world);

        let target = LayerPool::acquire_paint_target_in_world(&mut world, lease)
            .expect("offscreen apply should suppress instead of failing");

        assert!(
            target.is_none(),
            "offscreen policy should suppress the retained upload"
        );
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::PolicyChanged, 1),
                    (LayerPoolMetricName::UploadSuppressed, 1),
                ],
            );
        });
    }

    #[test]
    fn external_policy_bypasses_dom_invalidation_on_layer_updates() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        LayerPool::set_layer_policy_in_world(&mut world, lease, LayerPoolPolicy::External)
            .expect("external policy should update");
        clear_pending_jobs(&mut world);

        LayerPool::update_layer_lease_in_world(&mut world, lease, (3.0, 4.0, 64.0, 18.0))
            .expect("mapping update should still succeed");

        {
            let pending_jobs = world.resource::<LayerPoolRenderJobQueue>();
            assert_eq!(pending_jobs.jobs.len(), 0);
        }
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::PolicyChanged, 1),
                    (LayerPoolMetricName::LayerMappingUpdated, 1),
                    (LayerPoolMetricName::SurfaceGrew, 1),
                    (LayerPoolMetricName::InvalidationBypassed, 1),
                ],
            );
        });
    }

    #[test]
    fn external_policy_suppresses_retained_op_updates() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        LayerPool::set_layer_policy_in_world(&mut world, lease, LayerPoolPolicy::External)
            .expect("external policy should update");
        clear_pending_jobs(&mut world);

        let target = LayerPool::acquire_paint_target_in_world(&mut world, lease)
            .expect("external apply should suppress instead of failing");

        assert!(
            target.is_none(),
            "external policy should suppress the retained upload"
        );
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::PolicyChanged, 1),
                    (LayerPoolMetricName::UploadSuppressed, 1),
                ],
            );
        });
    }

    #[test]
    fn switching_to_external_invalidates_staged_dom_apply_work() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        let handler = Arc::new(RenderOpsHandler::new());
        register_render_ops_handler(&mut world, &handler);

        let target = LayerPool::acquire_paint_target_in_world(&mut world, lease)
            .expect("normal apply should acquire")
            .expect("normal policy should not suppress");
        let policy = target.policy;
        let _ = handler.stage(
            StagedLayerApply {
                lease,
                image_handle: target.image_handle,
                layer_zero_in_image_px: target.layer_zero_in_image_px,
                plan: single_rect_plan(),
            },
            policy,
        );
        LayerPool::set_layer_policy_in_world(&mut world, lease, LayerPoolPolicy::External)
            .expect("switching to external should still invalidate staged DOM work");

        handler.with_staged_applies(|applies| {
            assert!(
                applies.is_empty(),
                "switching to external should evict the staged DOM apply"
            );
        });
        let pending_jobs = world.resource::<LayerPoolRenderJobQueue>();
        assert!(
            matches!(
                pending_jobs.jobs.last(),
                Some(LayerPoolRenderJob::Invalidate { lease: invalidated_lease })
                    if *invalidated_lease == lease
            ),
            "switching to external should queue an invalidation for the executor"
        );
    }

    #[test]
    fn stale_apply_rejects_without_repaint_business_events() {
        let mut world = World::new();
        install_layer_pool(&mut world, PixelDensity::X1);
        let lease = LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 32.0, 16.0));
        let stale = LayerLease {
            layer_id: lease.layer_id,
            generation: lease
                .generation
                .checked_add(1)
                .expect("stale generation should remain within bounds"),
        };

        let apply_result = LayerPool::acquire_paint_target_in_world(&mut world, stale);

        assert_eq!(
            apply_result.expect_err("stale apply should be rejected"),
            LayerPoolApplyError::InvalidLease(
                "layer lease generation should match the current slot generation",
            )
        );
        world.resource_scope(|_, layer_pool: bevy::prelude::Mut<LayerPool>| {
            assert_metric_counts(
                &layer_pool,
                &[
                    (LayerPoolMetricName::LayerCreated, 1),
                    (LayerPoolMetricName::StaleLeaseRejected, 1),
                ],
            );
            assert_history_names(
                &layer_pool,
                &[
                    LayerPoolMetricName::LayerCreated,
                    LayerPoolMetricName::StaleLeaseRejected,
                ],
            );
        });
    }
}
