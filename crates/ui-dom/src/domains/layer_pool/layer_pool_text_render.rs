//! Render-only text-run helpers for the retained layer pool.
//!
//! This file exists so glyph-mask caching, text-buffer reuse, and final
//! text-run GPU submission stay on the render-owned side of the boundary
//! instead of drifting back into the shared `layer_pool` owner file.
//!
//! The moved state types (`FontRegistrationState`, `PreloadWarmupQueue`, and
//! `TextRunRenderState`) live in their own files and are re-exported here.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) use super::font_registration_state::FontRegistrationState;
pub(super) use super::preload_warmup_queue::PreloadWarmupQueue;
pub(super) use super::text_run_render_state::TextRunRenderState;

pub(super) const TEXT_SHAPE_RUN_CACHE_RETAINED_AGE_COUNT: u64 = 2;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct TextBufferKey {
    pub(super) text_run_id: TextRunId,
    pub(super) font_size_bits: u32,
    pub(super) line_height_bits: u32,
    pub(super) wrap_mode: TextWrapMode,
    pub(super) alignment: TextAlign,
    pub(super) width_bits: u32,
    pub(super) height_bits: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct GlyphMaskKey {
    pub(super) cache_key: CacheKey,
}

#[derive(Clone, Debug)]
pub(super) struct CachedTextBuffer {
    pub(super) buffer: Buffer,
}

pub(super) struct CachedGlyphMask {
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    width_px: u32,
    height_px: u32,
    placement_left_px: i32,
    placement_top_px: i32,
}

#[derive(Clone, Copy)]
pub(super) struct PreparedGlyphDraw {
    glyph_key: GlyphMaskKey,
    op: GpuTextRunOp,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum PreloadKey {
    GenericGlyphWarmup(GenericGlyphWarmupKey),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum GenericWarmupKind {
    GlyphMask,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct GenericGlyphWarmupKey {
    pub(super) font_face_id: FontFaceId,
    pub(super) language_hint_id: TextLanguageId,
    pub(super) font_size_bits: u32,
    pub(super) raster_scale_bits: u32,
    pub(super) warmup_kind: GenericWarmupKind,
    pub(super) sample_index: u16,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct GenericGlyphWarmupRequest {
    pub(super) key: GenericGlyphWarmupKey,
    pub(super) sample_text: &'static str,
}

pub(super) struct TextRunSurfaceContext<'a> {
    pub(super) device: &'a RenderDevice,
    pub(super) queue: &'a RenderQueue,
    pub(super) gpu_image: &'a GpuImage,
    pub(super) layer_zero_in_image_px: [f32; 2],
    pub(super) lease: LayerLease,
}

#[derive(Default)]
struct TextRunPreparationState {
    dependencies: PendingLeaseDependencies,
}

struct TextRunGpuState {
    pipeline: wgpu::RenderPipeline,
    gpu_buffer: wgpu::Buffer,
    gpu_capacity: usize,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    glyph_bind_group_layout: wgpu::BindGroupLayout,
}

struct TextRunPipelineState {
    pipeline: wgpu::RenderPipeline,
    uniform_bind_group_layout: wgpu::BindGroupLayout,
    glyph_bind_group_layout: wgpu::BindGroupLayout,
}

pub(super) enum GlyphMaskBuildResult {
    Cached(CachedGlyphMask),
    Skip,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GlyphMaskMissingMaskResult {
    Skip,
    Unsupported,
}

enum GlyphMaskCacheStatus {
    Ready { inserted: bool },
    Skip,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TextGlyphFallbackDecision {
    Continue,
    DebugRect,
}

pub(super) fn create_text_run_render_state(
    device: &RenderDevice,
    pixel_density: PixelDensity,
) -> TextRunRenderState {
    let gpu_state = create_text_run_gpu_state(device);
    TextRunRenderState {
        pixel_density,
        debug_pipeline: create_debug_rect_pipeline_state(
            device,
            "text_run",
            DIRECT_TEXT_DEBUG_SHADER,
        ),
        pipeline: gpu_state.pipeline,
        gpu_buffer: gpu_state.gpu_buffer,
        gpu_capacity: gpu_state.gpu_capacity,
        uniform_buffer: gpu_state.uniform_buffer,
        uniform_bind_group: gpu_state.uniform_bind_group,
        sampler: gpu_state.sampler,
        glyph_bind_group_layout: gpu_state.glyph_bind_group_layout,
        font_system: create_text_run_font_system(),
        font_catalog: text_layout::TextFontCatalog::repo_default(),
        swash_cache: SwashCache::new(),
        font_registration_state: FontRegistrationState::default(),
        run_bank: SecondaryMap::new(),
        text_buffer_cache: LruCache::default(),
        glyph_mask_cache: LruCache::default(),
        preload_warmup_queue: create_text_run_preload_queue(pixel_density),
        pending_lease_dependencies: HashMap::new(),
        prepared_glyph_draws: Vec::new(),
        fallback_debug_runs: Vec::new(),
    }
}

fn create_text_run_gpu_state(device: &RenderDevice) -> TextRunGpuState {
    let wgpu_device = device.wgpu_device();
    let pipeline_state = create_text_run_pipeline_state(wgpu_device);
    let (uniform_buffer, uniform_bind_group) =
        create_text_run_uniform_bind_group(wgpu_device, &pipeline_state.uniform_bind_group_layout);
    TextRunGpuState {
        pipeline: pipeline_state.pipeline,
        gpu_buffer: create_instance_buffer(
            device,
            1,
            std::mem::size_of::<GpuTextRunOp>(),
            "ui_dom_layer_pool_text_run_instances",
        ),
        gpu_capacity: 1,
        uniform_buffer,
        uniform_bind_group,
        sampler: create_text_run_sampler(wgpu_device),
        glyph_bind_group_layout: pipeline_state.glyph_bind_group_layout,
    }
}

fn create_text_run_pipeline_state(wgpu_device: &wgpu::Device) -> TextRunPipelineState {
    let shader = wgpu_device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ui_dom_layer_pool_text_run_shader"),
        source: wgpu::ShaderSource::Wgsl(DIRECT_TEXT_RUN_SHADER.into()),
    });
    let uniform_bind_group_layout =
        wgpu_device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui_dom_layer_pool_text_run_uniform_bind_group_layout"),
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
    let glyph_bind_group_layout =
        wgpu_device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui_dom_layer_pool_text_run_glyph_bind_group_layout"),
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
    let pipeline_layout = wgpu_device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ui_dom_layer_pool_text_run_pipeline_layout"),
        bind_group_layouts: &[&uniform_bind_group_layout, &glyph_bind_group_layout],
        push_constant_ranges: &[],
    });
    let pipeline = wgpu_device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ui_dom_layer_pool_text_run_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: &[gpu_text_run_op_layout()],
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
    TextRunPipelineState {
        pipeline,
        uniform_bind_group_layout,
        glyph_bind_group_layout,
    }
}

fn create_text_run_uniform_bind_group(
    wgpu_device: &wgpu::Device,
    uniform_bind_group_layout: &wgpu::BindGroupLayout,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let uniform_buffer = wgpu_device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui_dom_layer_pool_text_run_uniform_buffer"),
        size: u64::try_from(std::mem::size_of::<GpuTextRunPipelineUniform>())
            .expect("text run uniform size should fit into u64"),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniform_bind_group = wgpu_device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ui_dom_layer_pool_text_run_uniform_bind_group"),
        layout: uniform_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });
    (uniform_buffer, uniform_bind_group)
}

fn create_text_run_sampler(wgpu_device: &wgpu::Device) -> wgpu::Sampler {
    wgpu_device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("ui_dom_layer_pool_text_run_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    })
}

pub(super) fn create_text_run_font_system() -> FontSystem {
    let mut font_system = FontSystem::new();
    font_system
        .db_mut()
        .set_sans_serif_family(text_layout::text_render::repo_default_sans_serif_family_name());
    font_system
}

pub(super) fn create_text_run_preload_queue(pixel_density: PixelDensity) -> PreloadWarmupQueue {
    let mut preload_warmup_queue = PreloadWarmupQueue::default();
    for language_hint_id in [
        TextLanguageId::English,
        TextLanguageId::Japanese,
        TextLanguageId::SimplifiedChinese,
        TextLanguageId::Korean,
        TextLanguageId::Thai,
    ] {
        enqueue_language_glyph_warmups(&mut preload_warmup_queue, language_hint_id, pixel_density);
    }
    preload_warmup_queue
}

fn enqueue_language_glyph_warmups(
    preload_warmup_queue: &mut PreloadWarmupQueue,
    language_hint_id: TextLanguageId,
    pixel_density: PixelDensity,
) {
    for (sample_index, sample_text) in language_warmup_samples(language_hint_id).iter().enumerate()
    {
        enqueue_glyph_mask_warmups_for_sample(
            preload_warmup_queue,
            language_hint_id,
            sample_text,
            sample_index,
            pixel_density,
        );
    }
}

fn enqueue_glyph_mask_warmups_for_sample(
    preload_warmup_queue: &mut PreloadWarmupQueue,
    language_hint_id: TextLanguageId,
    sample_text: &'static str,
    sample_index: usize,
    pixel_density: PixelDensity,
) {
    let sample_index =
        u16::try_from(sample_index).expect("text warmup sample index should fit into u16");
    for font_size_px in UI_TEXT_PRELOAD_FONT_SIZES_PX {
        preload_warmup_queue.push(GenericGlyphWarmupRequest {
            key: generic_glyph_warmup_key(
                language_hint_id,
                font_size_px,
                sample_index,
                pixel_density,
            ),
            sample_text,
        });
    }
}

fn generic_glyph_warmup_key(
    language_hint_id: TextLanguageId,
    font_size_px: f32,
    sample_index: u16,
    pixel_density: PixelDensity,
) -> GenericGlyphWarmupKey {
    GenericGlyphWarmupKey {
        font_face_id: FontFaceId::EMBEDDED_IBM_PLEX_SANS,
        language_hint_id,
        font_size_bits: font_size_px.to_bits(),
        raster_scale_bits: pixel_density_scale(pixel_density).to_bits(),
        warmup_kind: GenericWarmupKind::GlyphMask,
        sample_index,
    }
}

fn language_warmup_samples(language_hint_id: TextLanguageId) -> &'static [&'static str] {
    match language_hint_id {
        TextLanguageId::English => &UI_TEXT_ENGLISH_WARMUP_SAMPLES,
        TextLanguageId::Japanese => &UI_TEXT_JAPANESE_WARMUP_SAMPLES,
        TextLanguageId::SimplifiedChinese => &UI_TEXT_SIMPLIFIED_CHINESE_WARMUP_SAMPLES,
        TextLanguageId::Korean => &UI_TEXT_KOREAN_WARMUP_SAMPLES,
        TextLanguageId::Thai => &UI_TEXT_THAI_WARMUP_SAMPLES,
    }
}

pub(super) fn apply_text_runs_to_surface(
    text_run_state: &mut TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
    text_runs: &[TextRunPaintOp],
) -> bool {
    text_run_state.run_preload_warmups(context.device, context.queue);
    reset_text_run_surface_state(text_run_state);
    let mut preparation = TextRunPreparationState::default();
    for text_run in text_runs {
        prepare_text_run_for_surface(text_run_state, context, text_run, &mut preparation);
    }
    if text_run_dependencies_are_pending(&preparation.dependencies) {
        hold_text_runs_until_dependencies_arrive(text_run_state, context, preparation.dependencies);
        return false;
    }
    text_run_state
        .pending_lease_dependencies
        .remove(&context.lease);
    if !text_run_state.prepared_glyph_draws.is_empty() {
        let _ = ensure_text_run_gpu_capacity(
            context.device,
            text_run_state,
            text_run_state.prepared_glyph_draws.len(),
        );
    }
    write_text_run_uniforms(text_run_state, context);
    draw_prepared_text_runs_to_surface(text_run_state, context);
    if !text_run_state.fallback_debug_runs.is_empty() {
        let _ = draw_text_run_fallback_debug_rects(text_run_state, context);
    }
    true
}

fn reset_text_run_surface_state(text_run_state: &mut TextRunRenderState) {
    text_run_state.prepared_glyph_draws.clear();
    text_run_state.fallback_debug_runs.clear();
}

fn prepare_text_run_for_surface(
    text_run_state: &mut TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
    text_run: &TextRunPaintOp,
    preparation: &mut TextRunPreparationState,
) {
    let Some(run_record) = text_run_record_for_surface(text_run_state, text_run, preparation)
    else {
        return;
    };
    let run_start = text_run_state.prepared_glyph_draws.len();
    let buffer_key = text_buffer_key(text_run);
    ensure_text_buffer_cached(text_run_state, &run_record, text_run, buffer_key);
    let cached_buffer = text_run_state
        .text_buffer_cache
        .remove(&buffer_key)
        .expect("cached text buffer should exist after insertion");
    let fallback_to_debug =
        prepare_text_buffer_glyphs(text_run_state, context, text_run, &cached_buffer);
    let _ = text_run_state
        .text_buffer_cache
        .insert(&buffer_key, cached_buffer);
    if fallback_to_debug {
        text_run_state.prepared_glyph_draws.truncate(run_start);
        text_run_state.fallback_debug_runs.push(*text_run);
    }
}

fn text_run_record_for_surface(
    text_run_state: &TextRunRenderState,
    text_run: &TextRunPaintOp,
    preparation: &mut TextRunPreparationState,
) -> Option<RenderTextRunRecord> {
    let Some(run_record) = text_run_state.run_bank.get(text_run.text_run_id).cloned() else {
        push_unique_copy(
            &mut preparation.dependencies.missing_text_runs,
            text_run.text_run_id,
        );
        return None;
    };
    let face_id = text_layout::text_render::resolve_face_id(
        &run_record.family,
        run_record.language_hint_id,
        &text_run_state.font_catalog,
    );
    if !text_run_state.font_registration_state.ready(face_id) {
        push_unique_copy(&mut preparation.dependencies.missing_font_faces, face_id);
        return None;
    }
    Some(run_record)
}

fn ensure_text_buffer_cached(
    text_run_state: &mut TextRunRenderState,
    run_record: &RenderTextRunRecord,
    text_run: &TextRunPaintOp,
    buffer_key: TextBufferKey,
) {
    if text_run_state.text_buffer_cache.contains(&buffer_key) {
        return;
    }
    let cached_text_buffer = build_text_buffer(
        &mut text_run_state.font_system,
        &text_run_state.font_catalog,
        run_record,
        text_run,
    );
    let _ = text_run_state
        .text_buffer_cache
        .insert(&buffer_key, cached_text_buffer);
}

fn prepare_text_buffer_glyphs(
    text_run_state: &mut TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
    text_run: &TextRunPaintOp,
    cached_buffer: &CachedTextBuffer,
) -> bool {
    for run in cached_buffer.buffer.layout_runs() {
        let baseline_y_px = text_run.position_px[1] + run.line_y;
        for glyph in run.glyphs {
            if glyph_requires_debug_fallback(glyph.glyph_id) == TextGlyphFallbackDecision::DebugRect
            {
                return true;
            }
            let physical_glyph = glyph_in_retained_pixels(
                glyph,
                text_run,
                baseline_y_px,
                text_run_state.pixel_density,
            );
            let glyph_key = GlyphMaskKey {
                cache_key: physical_glyph.cache_key,
            };
            match ensure_glyph_mask_cached(
                text_run_state,
                context.device,
                context.queue,
                glyph_key,
                physical_glyph.cache_key,
            ) {
                GlyphMaskCacheStatus::Ready { inserted } => {
                    let _ = inserted;
                }
                GlyphMaskCacheStatus::Skip => continue,
                GlyphMaskCacheStatus::Unsupported => return true,
            }
            let cached_glyph = text_run_state
                .glyph_mask_cache
                .get(&glyph_key)
                .expect("cached glyph mask should exist after insertion");
            let Some(op) = build_text_glyph_op(
                text_run,
                physical_glyph.x + cached_glyph.placement_left_px,
                physical_glyph.y - cached_glyph.placement_top_px,
                cached_glyph.width_px,
                cached_glyph.height_px,
            ) else {
                continue;
            };
            text_run_state
                .prepared_glyph_draws
                .push(PreparedGlyphDraw { glyph_key, op });
        }
    }
    false
}

pub(super) fn glyph_in_retained_pixels(
    glyph: &LayoutGlyph,
    text_run: &TextRunPaintOp,
    baseline_y_px: f32,
    pixel_density: PixelDensity,
) -> PhysicalGlyph {
    let density_scale = pixel_density_scale(pixel_density);
    glyph.physical(
        (
            text_run.position_px[0] * density_scale,
            baseline_y_px * density_scale,
        ),
        density_scale,
    )
}

pub(super) fn glyph_requires_debug_fallback(glyph_id: u16) -> TextGlyphFallbackDecision {
    if glyph_id == 0 {
        return TextGlyphFallbackDecision::DebugRect;
    }
    TextGlyphFallbackDecision::Continue
}

fn ensure_glyph_mask_cached(
    text_run_state: &mut TextRunRenderState,
    device: &RenderDevice,
    queue: &RenderQueue,
    glyph_key: GlyphMaskKey,
    cache_key: CacheKey,
) -> GlyphMaskCacheStatus {
    if text_run_state.glyph_mask_cache.contains(&glyph_key) {
        let _ = text_run_state.glyph_mask_cache.get(&glyph_key);
        return GlyphMaskCacheStatus::Ready { inserted: false };
    }
    match build_glyph_mask(
        device,
        queue,
        &text_run_state.glyph_bind_group_layout,
        &text_run_state.sampler,
        &mut text_run_state.font_system,
        &mut text_run_state.swash_cache,
        cache_key,
    ) {
        GlyphMaskBuildResult::Cached(cached_glyph_mask) => {
            let _ = text_run_state
                .glyph_mask_cache
                .insert(&glyph_key, cached_glyph_mask);
            GlyphMaskCacheStatus::Ready { inserted: true }
        }
        GlyphMaskBuildResult::Skip => GlyphMaskCacheStatus::Skip,
        GlyphMaskBuildResult::Unsupported => GlyphMaskCacheStatus::Unsupported,
    }
}

fn text_run_dependencies_are_pending(dependencies: &PendingLeaseDependencies) -> bool {
    !dependencies.missing_text_runs.is_empty() || !dependencies.missing_font_faces.is_empty()
}

fn hold_text_runs_until_dependencies_arrive(
    text_run_state: &mut TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
    dependencies: PendingLeaseDependencies,
) {
    text_run_state
        .pending_lease_dependencies
        .insert(context.lease, dependencies);
    reset_text_run_surface_state(text_run_state);
}

fn write_text_run_uniforms(
    text_run_state: &TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
) {
    context.queue.write_buffer(
        &text_run_state.uniform_buffer,
        0,
        bytemuck::bytes_of(&GpuTextRunPipelineUniform {
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
        }),
    );
}

fn draw_prepared_text_runs_to_surface(
    text_run_state: &mut TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
) {
    if text_run_state.prepared_glyph_draws.is_empty() {
        return;
    }
    write_prepared_text_run_ops(text_run_state, context.queue);
    let mut encoder =
        context
            .device
            .wgpu_device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ui_dom_layer_pool_text_run_encoder"),
            });
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui_dom_layer_pool_text_run_pass"),
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
        render_pass.set_pipeline(&text_run_state.pipeline);
        render_pass.set_bind_group(0, &text_run_state.uniform_bind_group, &[]);
        render_pass.set_vertex_buffer(0, text_run_state.gpu_buffer.slice(..));
        for (index, prepared_glyph) in text_run_state.prepared_glyph_draws.iter().enumerate() {
            let cached_glyph = text_run_state
                .glyph_mask_cache
                .get(&prepared_glyph.glyph_key)
                .expect("prepared glyph should stay cached until draw");
            let _ = &cached_glyph.texture;
            render_pass.set_bind_group(1, &cached_glyph.bind_group, &[]);
            let instance_index =
                u32::try_from(index).expect("prepared glyph count should fit into u32");
            render_pass.draw(0..6, instance_index..instance_index.wrapping_add(1));
        }
    }
    context.queue.submit(std::iter::once(encoder.finish()));
}

fn write_prepared_text_run_ops(text_run_state: &TextRunRenderState, queue: &RenderQueue) {
    for (index, prepared_glyph) in text_run_state.prepared_glyph_draws.iter().enumerate() {
        let byte_offset = u64::try_from(index)
            .expect("prepared glyph count should fit into u64")
            .checked_mul(
                u64::try_from(std::mem::size_of::<GpuTextRunOp>())
                    .expect("text glyph instance stride should fit into u64"),
            )
            .expect("prepared glyph byte offset should stay in range");
        queue.write_buffer(
            &text_run_state.gpu_buffer,
            byte_offset,
            bytemuck::bytes_of(&prepared_glyph.op),
        );
    }
}

fn draw_text_run_fallback_debug_rects(
    text_run_state: &mut TextRunRenderState,
    context: &TextRunSurfaceContext<'_>,
) -> bool {
    apply_debug_rect_ops_to_surface(
        &mut text_run_state.debug_pipeline,
        &DebugRectSurfaceContext {
            device: context.device,
            queue: context.queue,
            gpu_image: context.gpu_image,
            pixel_density: text_run_state.pixel_density,
            op_count: text_run_state.fallback_debug_runs.len(),
            layer_zero_in_image_px: context.layer_zero_in_image_px,
            pipeline_name: "text_run",
        },
        |gpu_buffer, queue| {
            for (index, text_run) in text_run_state.fallback_debug_runs.iter().enumerate() {
                write_debug_rect_op(
                    gpu_buffer,
                    queue,
                    index,
                    GpuDebugRectOp {
                        position_px: text_run.position_px,
                        size_px: text_run.size_px,
                        fill_rgba: text_run.color_rgba,
                    },
                );
            }
        },
    )
}

fn ensure_text_run_gpu_capacity(
    device: &RenderDevice,
    text_run_state: &mut TextRunRenderState,
    instance_count: usize,
) -> bool {
    if instance_count <= text_run_state.gpu_capacity {
        return false;
    }
    let required_capacity = instance_count.next_power_of_two();
    text_run_state.gpu_buffer = create_instance_buffer(
        device,
        required_capacity,
        std::mem::size_of::<GpuTextRunOp>(),
        "ui_dom_layer_pool_text_run_instances",
    );
    text_run_state.gpu_capacity = required_capacity;
    true
}

pub(super) fn text_buffer_key(text_run: &TextRunPaintOp) -> TextBufferKey {
    TextBufferKey {
        text_run_id: text_run.text_run_id,
        font_size_bits: text_run.font_size_px.to_bits(),
        line_height_bits: text_run.line_height_px.to_bits(),
        wrap_mode: text_run.wrap_mode,
        alignment: text_run.alignment,
        width_bits: text_run.size_px[0].to_bits(),
        height_bits: text_run.size_px[1].to_bits(),
    }
}

pub(super) fn build_text_buffer(
    font_system: &mut FontSystem,
    font_catalog: &text_layout::TextFontCatalog,
    run_record: &RenderTextRunRecord,
    text_run: &TextRunPaintOp,
) -> CachedTextBuffer {
    let request = text_layout::TextLayoutRequest {
        text: run_record.text.as_ref(),
        white_space: DomWhiteSpace::Pre,
        word_break: crate::types::DomWordBreak::Normal,
        family: run_record.family.clone(),
        language_hint_id: run_record.language_hint_id,
        weight: run_record.weight,
        style: run_record.style,
        direction: crate::types::DomTextDirection::Ltr,
        font_size_px: text_run.font_size_px,
        line_height_px: Some(text_run.line_height_px),
        width_px: Some(text_run.size_px[0]),
        wrap_mode: text_run.wrap_mode,
        alignment: text_run.alignment,
    };
    CachedTextBuffer {
        buffer: text_layout::text_render::build_render_text_buffer(
            font_system,
            font_catalog,
            &request,
            Some(text_run.size_px[1]),
        ),
    }
}

pub(super) fn build_glyph_mask(
    device: &RenderDevice,
    queue: &RenderQueue,
    glyph_bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cache_key: CacheKey,
) -> GlyphMaskBuildResult {
    let Some(image) = swash_cache.get_image(font_system, cache_key).clone() else {
        return glyph_mask_result_without_mask(font_system, swash_cache, cache_key);
    };
    if !matches!(image.content, SwashContent::Mask) {
        return GlyphMaskBuildResult::Unsupported;
    }
    let Some(texture_extent) =
        glyph_mask_texture_extent(image.placement.width, image.placement.height)
    else {
        return glyph_mask_result_without_mask(font_system, swash_cache, cache_key);
    };
    let texture = device
        .wgpu_device()
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("ui_dom_layer_pool_text_run_glyph_mask"),
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
        &image.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.placement.width),
            rows_per_image: Some(image.placement.height),
        },
        texture_extent,
    );
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device
        .wgpu_device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui_dom_layer_pool_text_run_glyph_bind_group"),
            layout: glyph_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
    GlyphMaskBuildResult::Cached(CachedGlyphMask {
        bind_group,
        texture,
        width_px: image.placement.width,
        height_px: image.placement.height,
        placement_left_px: image.placement.left,
        placement_top_px: image.placement.top,
    })
}

fn glyph_mask_result_without_mask(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cache_key: CacheKey,
) -> GlyphMaskBuildResult {
    match glyph_mask_missing_mask_result(glyph_has_outline_commands(
        font_system,
        swash_cache,
        cache_key,
    )) {
        GlyphMaskMissingMaskResult::Skip => GlyphMaskBuildResult::Skip,
        GlyphMaskMissingMaskResult::Unsupported => GlyphMaskBuildResult::Unsupported,
    }
}

pub(super) fn glyph_mask_missing_mask_result(
    has_outline_commands: bool,
) -> GlyphMaskMissingMaskResult {
    if has_outline_commands {
        return GlyphMaskMissingMaskResult::Unsupported;
    }
    GlyphMaskMissingMaskResult::Skip
}

fn glyph_has_outline_commands(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cache_key: CacheKey,
) -> bool {
    swash_cache
        .get_outline_commands(font_system, cache_key)
        .is_some_and(|commands| !commands.is_empty())
}

pub(super) fn glyph_mask_texture_extent(width_px: u32, height_px: u32) -> Option<Extent3d> {
    if width_px == 0 || height_px == 0 {
        return None;
    }
    Some(Extent3d {
        width: width_px,
        height: height_px,
        depth_or_array_layers: 1,
    })
}

fn build_text_glyph_op(
    text_run: &TextRunPaintOp,
    glyph_left_px: i32,
    glyph_top_px: i32,
    glyph_width_px: u32,
    glyph_height_px: u32,
) -> Option<GpuTextRunOp> {
    if glyph_width_px == 0 || glyph_height_px == 0 {
        return None;
    }
    Some(GpuTextRunOp {
        position_px: [i32_to_f32(glyph_left_px), i32_to_f32(glyph_top_px)],
        size_px: [u32_to_f32(glyph_width_px), u32_to_f32(glyph_height_px)],
        color_rgba: text_run.color_rgba,
    })
}

fn push_unique_copy<T>(items: &mut Vec<T>, item: T)
where
    T: Copy + Eq,
{
    if items.contains(&item) {
        return;
    }
    items.push(item);
}
