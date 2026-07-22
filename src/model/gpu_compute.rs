use std::sync::{mpsc, Arc, LazyLock, RwLock};

use bytemuck::{Pod, Zeroable};
use color_eyre::eyre::{eyre, Result};
use wgpu::util::DeviceExt as _;

use super::{ComparisonMode, ImageSpec, MeanDim, PixelType, Recti};
use crate::util::math_ext::Vec2i;

const WORKGROUP_SIZE: u32 = 256;
const REDUCTION_GROUPS: u32 = 1024;

static GPU_COMPUTE: LazyLock<RwLock<Option<Arc<GpuComputeContext>>>> = LazyLock::new(|| RwLock::new(None));

pub fn install_gpu_compute(device: &wgpu::Device, queue: &wgpu::Queue, stream_rgba_uploads: bool) {
    let mut slot = GPU_COMPUTE.write().unwrap();
    if slot.is_none() {
        *slot = Some(Arc::new(GpuComputeContext::new(
            device.clone(),
            queue.clone(),
            stream_rgba_uploads,
        )));
    }
}

pub fn gpu_compute() -> Result<Arc<GpuComputeContext>> {
    GPU_COMPUTE
        .read()
        .unwrap()
        .clone()
        .ok_or_else(|| eyre!("GPU compute is not initialized yet"))
}

#[derive(Clone)]
pub struct GpuImageTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub spec: ImageSpec,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ComputeParams {
    roi: [u32; 4],
    image: [u32; 4],
    operation: [u32; 4],
    values: [f32; 4],
}

impl ComputeParams {
    fn new(spec: &ImageSpec, rect: Recti) -> Self {
        let rect = normalized_rect(spec, rect);
        let (x, y, width, height) = rect.xywh();
        Self {
            roi: [x as u32, y as u32, width as u32, height as u32],
            image: [spec.width as u32, spec.height as u32, spec.channels as u32, 0],
            operation: [0; 4],
            values: [0.0; 4],
        }
    }
}

pub struct GpuComputeContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    compute_module: wgpu::ShaderModule,
    compute_pipeline_layout: wgpu::PipelineLayout,
    minmax_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    histogram_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    sum_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    vector_mean_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    compare_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    ssim_pipeline: std::sync::OnceLock<wgpu::ComputePipeline>,
    rgb_upload_layout: wgpu::BindGroupLayout,
    rgb_upload_pipeline: wgpu::ComputePipeline,
    stream_rgba_uploads: bool,
    dummy_source: GpuImageTexture,
    dummy_target: GpuImageTexture,
}

impl GpuComputeContext {
    fn new(device: wgpu::Device, queue: wgpu::Queue, stream_rgba_uploads: bool) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edolview image compute layout"),
            entries: &[
                texture_layout_entry(0),
                texture_layout_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let compute_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edolview image compute shader"),
            source: wgpu::ShaderSource::Wgsl(COMPUTE_SHADER.into()),
        });
        let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edolview image compute pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let rgb_upload_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edolview RGB upload layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let rgb_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edolview RGB upload shader"),
            source: wgpu::ShaderSource::Wgsl(RGB_UPLOAD_SHADER.into()),
        });
        let rgb_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edolview RGB upload pipeline layout"),
            bind_group_layouts: &[Some(&rgb_upload_layout)],
            immediate_size: 0,
        });
        let rgb_upload_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("edolview RGB upload pipeline"),
            layout: Some(&rgb_pipeline_layout),
            module: &rgb_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let dummy_source = create_empty_rgba_texture(&device, 1, 1, 4, "edolview compute dummy source");
        let dummy_target = create_empty_rgba_texture(&device, 1, 1, 4, "edolview compute dummy target");
        Self {
            device,
            queue,
            layout,
            compute_module,
            compute_pipeline_layout,
            minmax_pipeline: std::sync::OnceLock::new(),
            histogram_pipeline: std::sync::OnceLock::new(),
            sum_pipeline: std::sync::OnceLock::new(),
            vector_mean_pipeline: std::sync::OnceLock::new(),
            compare_pipeline: std::sync::OnceLock::new(),
            ssim_pipeline: std::sync::OnceLock::new(),
            rgb_upload_layout,
            rgb_upload_pipeline,
            stream_rgba_uploads,
            dummy_source,
            dummy_target,
        }
    }

    pub fn upload(&self, spec: &ImageSpec, pixels: &[f32]) -> Result<Arc<GpuImageTexture>> {
        if spec.width <= 0 || spec.height <= 0 || !(1..=4).contains(&spec.channels) {
            return Err(eyre!("Invalid image dimensions or channels"));
        }
        let expected = spec.width as usize * spec.height as usize * spec.channels as usize;
        if pixels.len() != expected {
            return Err(eyre!("Unexpected image data length: {} != {expected}", pixels.len()));
        }
        let format = texture_format(spec.channels)?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edolview shared image"),
            size: wgpu::Extent3d {
                width: spec.width as u32,
                height: spec.height as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | if spec.channels == 3 {
                    wgpu::TextureUsages::STORAGE_BINDING
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        });
        if spec.channels == 3 {
            self.upload_rgb(&texture, pixels)?;
        } else {
            self.upload_direct(&texture, spec, pixels)?;
        }
        Ok(Arc::new(GpuImageTexture {
            view: texture.create_view(&Default::default()),
            texture,
            spec: spec.clone(),
        }))
    }

    fn upload_direct(&self, texture: &wgpu::Texture, spec: &ImageSpec, pixels: &[f32]) -> Result<()> {
        const DX12_CHUNK_BYTES: u64 = 16 * 1024 * 1024;
        let bytes = bytemuck::cast_slice(pixels);
        let bytes_per_row = spec.width as u32 * spec.channels as u32 * 4;
        let should_stream = self.stream_rgba_uploads && spec.channels == 4 && bytes.len() as u64 > DX12_CHUNK_BYTES;
        let rows_per_chunk = if should_stream {
            (DX12_CHUNK_BYTES / u64::from(bytes_per_row)).max(1) as u32
        } else {
            spec.height as u32
        };
        for base_y in (0..spec.height as u32).step_by(rows_per_chunk as usize) {
            let row_count = rows_per_chunk.min(spec.height as u32 - base_y);
            let start = base_y as usize * bytes_per_row as usize;
            let end = (base_y + row_count) as usize * bytes_per_row as usize;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: base_y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes[start..end],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(row_count),
                },
                wgpu::Extent3d {
                    width: spec.width as u32,
                    height: row_count,
                    depth_or_array_layers: 1,
                },
            );
            if should_stream {
                self.queue.submit([]);
                self.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .map_err(|error| eyre!("Failed to flush an image upload chunk: {error}"))?;
            }
        }
        Ok(())
    }

    fn upload_rgb(&self, texture: &wgpu::Texture, pixels: &[f32]) -> Result<()> {
        let size = texture.size();
        let rows_per_chunk =
            rgb_rows_per_chunk(size.width, self.device.limits().max_storage_buffer_binding_size as u64)
                .min(size.height);
        let view = texture.create_view(&Default::default());
        for base_y in (0..size.height).step_by(rows_per_chunk as usize) {
            let row_count = rows_per_chunk.min(size.height - base_y);
            let start = base_y as usize * size.width as usize * 3;
            let end = (base_y + row_count) as usize * size.width as usize * 3;
            let input = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("edolview RGB upload buffer"),
                contents: bytemuck::cast_slice(&pixels[start..end]),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let parameters = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("edolview RGB upload parameters"),
                contents: bytemuck::cast_slice(&[base_y, size.width, row_count, 0u32]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("edolview RGB upload bind group"),
                layout: &self.rgb_upload_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: parameters.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("edolview RGB upload encoder"),
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("edolview RGB upload pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.rgb_upload_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(size.width.div_ceil(16), row_count.div_ceil(16), 1);
            }
            self.queue.submit([encoder.finish()]);
        }
        Ok(())
    }

    pub fn compare(
        &self,
        lhs: &GpuImageTexture,
        rhs: &GpuImageTexture,
        output_spec: &ImageSpec,
        mode: ComparisonMode,
        alpha: f32,
        strategy: u32,
    ) -> Result<Arc<GpuImageTexture>> {
        let output = create_empty_rgba_texture(
            &self.device,
            output_spec.width as u32,
            output_spec.height as u32,
            output_spec.channels,
            "edolview comparison image",
        );
        let mut params = ComputeParams::new(output_spec, Recti::ZERO);
        params.operation = [
            match mode {
                ComparisonMode::Diff => 0,
                ComparisonMode::Blend => 1,
                ComparisonMode::Split => 2,
            },
            lhs.spec.channels as u32,
            rhs.spec.channels as u32,
            strategy,
        ];
        params.values[0] = alpha.clamp(0.0, 1.0);
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edolview comparison dummy output"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let (params_buffer, bind_group) = self.bind(lhs, Some(rhs), &output_buffer, &params, Some(&output));
        self.dispatch_2d(
            self.compute_pipeline(&self.compare_pipeline, "compare_images"),
            &bind_group,
            (output_spec.width as u32).div_ceil(16),
            (output_spec.height as u32).div_ceil(16),
        );
        drop(params_buffer);
        Ok(Arc::new(output))
    }

    pub fn minmax(&self, image: &GpuImageTexture, rect: Recti) -> Result<(Vec<f32>, Vec<f32>)> {
        let channels = image.spec.channels as usize;
        let init = [u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0, 0, 0, 0];
        let output = self.storage_buffer(bytemuck::cast_slice(&init), "edolview minmax output");
        let params = ComputeParams::new(&image.spec, rect);
        let groups = reduction_group_count(&params);
        self.run_readback(
            self.compute_pipeline(&self.minmax_pipeline, "minmax"),
            image,
            None,
            &output,
            &params,
            groups,
            1,
            32,
            |bytes| {
                let words: &[u32] = bytemuck::cast_slice(bytes);
                let mut mins = Vec::with_capacity(channels);
                let mut maxs = Vec::with_capacity(channels);
                for channel in 0..channels {
                    mins.push(ordered_to_float(words[channel]));
                    maxs.push(ordered_to_float(words[4 + channel]));
                }
                (mins, maxs)
            },
        )
    }

    pub fn histogram(&self, image: &GpuImageTexture) -> Result<Vec<Vec<f32>>> {
        let size = 4 * 256 * std::mem::size_of::<u32>();
        let zeros = vec![0u8; size];
        let output = self.storage_buffer(&zeros, "edolview histogram output");
        let params = ComputeParams::new(&image.spec, Recti::ZERO);
        let groups = reduction_group_count(&params);
        self.run_readback(
            self.compute_pipeline(&self.histogram_pipeline, "histogram"),
            image,
            None,
            &output,
            &params,
            groups,
            1,
            size as u64,
            |bytes| {
                let words: &[u32] = bytemuck::cast_slice(bytes);
                (0..image.spec.channels as usize)
                    .map(|channel| words[channel * 256..(channel + 1) * 256].iter().map(|&v| v as f32).collect())
                    .collect()
            },
        )
    }

    pub fn mean(&self, image: &GpuImageTexture, rect: Recti, dim: MeanDim) -> Result<Vec<f64>> {
        let rect = normalized_rect(&image.spec, rect);
        if rect.empty() {
            return Ok(Vec::new());
        }
        match dim {
            MeanDim::All => {
                let mut params = ComputeParams::new(&image.spec, rect);
                params.operation[0] = 0;
                let groups = reduction_group_count(&params);
                let size = groups as u64 * 16;
                let output = self.empty_storage_buffer(size, "edolview mean partials");
                self.run_readback(
                    self.compute_pipeline(&self.sum_pipeline, "sum_values"),
                    image,
                    None,
                    &output,
                    &params,
                    groups,
                    1,
                    size,
                    |bytes| {
                        let partials: &[f32] = bytemuck::cast_slice(bytes);
                        let count = (params.roi[2] as f64) * (params.roi[3] as f64);
                        (0..image.spec.channels as usize)
                            .map(|channel| {
                                partials.chunks_exact(4).map(|partial| partial[channel] as f64).sum::<f64>() / count
                            })
                            .collect()
                    },
                )
            }
            MeanDim::Column | MeanDim::Row => {
                let mut params = ComputeParams::new(&image.spec, rect);
                params.operation[0] = u32::from(matches!(dim, MeanDim::Row));
                let count = if matches!(dim, MeanDim::Column) {
                    params.roi[2]
                } else {
                    params.roi[3]
                };
                let size = count as u64 * 16;
                let output = self.empty_storage_buffer(size, "edolview vector mean output");
                self.run_readback(
                    self.compute_pipeline(&self.vector_mean_pipeline, "vector_mean"),
                    image,
                    None,
                    &output,
                    &params,
                    count,
                    1,
                    size,
                    |bytes| {
                        let values: &[f32] = bytemuck::cast_slice(bytes);
                        values
                            .chunks_exact(4)
                            .flat_map(|pixel| pixel[..image.spec.channels as usize].iter().map(|&v| v as f64))
                            .collect()
                    },
                )
            }
        }
    }

    pub fn psnr(
        &self,
        lhs: &GpuImageTexture,
        rhs: &GpuImageTexture,
        rect: Recti,
        data_range: f64,
        scale: f64,
    ) -> Result<(f64, f64)> {
        ensure_compatible(lhs, rhs)?;
        let mut params = ComputeParams::new(&lhs.spec, rect);
        params.operation[0] = 1;
        let groups = reduction_group_count(&params);
        let size = groups as u64 * 16;
        let output = self.empty_storage_buffer(size, "edolview squared error partials");
        self.run_readback(
            self.compute_pipeline(&self.sum_pipeline, "sum_values"),
            lhs,
            Some(rhs),
            &output,
            &params,
            groups,
            1,
            size,
            |bytes| {
                let values: &[f32] = bytemuck::cast_slice(bytes);
                let squared_error: f64 = values.iter().map(|&v| v as f64).sum();
                let sample_count = params.roi[2] as f64 * params.roi[3] as f64 * lhs.spec.channels as f64;
                let mse = squared_error / sample_count;
                let psnr = if mse == 0.0 {
                    f64::INFINITY
                } else {
                    10.0 * ((data_range * data_range) / mse).log10()
                };
                let rmse = scale * mse.sqrt();
                (psnr, rmse)
            },
        )
    }

    pub fn ssim(&self, lhs: &GpuImageTexture, rhs: &GpuImageTexture, rect: Recti) -> Result<f64> {
        ensure_compatible(lhs, rhs)?;
        let params = ComputeParams::new(&lhs.spec, rect);
        let groups = reduction_group_count(&params);
        let size = groups as u64 * 16;
        let output = self.empty_storage_buffer(size, "edolview ssim partials");
        self.run_readback(
            self.compute_pipeline(&self.ssim_pipeline, "ssim"),
            lhs,
            Some(rhs),
            &output,
            &params,
            groups,
            1,
            size,
            |bytes| {
                let values: &[f32] = bytemuck::cast_slice(bytes);
                let sum: f64 = values.iter().map(|&v| v as f64).sum();
                let count = params.roi[2] as f64 * params.roi[3] as f64 * lhs.spec.channels as f64;
                sum / count
            },
        )
    }

    pub fn wait_idle(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| eyre!("GPU wait failed: {error}"))?;
        Ok(())
    }

    fn compute_pipeline<'a>(
        &'a self,
        slot: &'a std::sync::OnceLock<wgpu::ComputePipeline>,
        entry_point: &'static str,
    ) -> &'a wgpu::ComputePipeline {
        slot.get_or_init(|| {
            self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&self.compute_pipeline_layout),
                module: &self.compute_module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        })
    }

    fn bind(
        &self,
        image: &GpuImageTexture,
        second: Option<&GpuImageTexture>,
        output: &wgpu::Buffer,
        params: &ComputeParams,
        target: Option<&GpuImageTexture>,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let params_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("edolview compute parameters"),
            contents: bytemuck::bytes_of(params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edolview image compute bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&image.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        second.map_or(&self.dummy_source.view, |texture| &texture.view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        target.map_or(&self.dummy_target.view, |texture| &texture.view),
                    ),
                },
            ],
        });
        (params_buffer, bind_group)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_readback<T>(
        &self,
        pipeline: &wgpu::ComputePipeline,
        image: &GpuImageTexture,
        second: Option<&GpuImageTexture>,
        output: &wgpu::Buffer,
        params: &ComputeParams,
        groups_x: u32,
        groups_y: u32,
        output_size: u64,
        decode: impl FnOnce(&[u8]) -> T,
    ) -> Result<T> {
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edolview compute readback"),
            size: output_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let (_params_buffer, bind_group) = self.bind(image, second, output, params, None);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("edolview image compute encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("edolview image compute pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(groups_x.max(1), groups_y.max(1), 1);
        }
        encoder.copy_buffer_to_buffer(output, 0, &readback, 0, output_size);
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| eyre!("GPU compute poll failed: {error}"))?;
        rx.recv().map_err(|_| eyre!("GPU readback callback disconnected"))??;
        let mapped = slice.get_mapped_range();
        let result = decode(&mapped);
        drop(mapped);
        readback.unmap();
        Ok(result)
    }

    fn dispatch_2d(&self, pipeline: &wgpu::ComputePipeline, bind_group: &wgpu::BindGroup, x: u32, y: u32) {
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("edolview image generation encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("edolview image generation pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(x.max(1), y.max(1), 1);
        }
        self.queue.submit([encoder.finish()]);
    }

    fn storage_buffer(&self, contents: &[u8], label: &'static str) -> wgpu::Buffer {
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        })
    }

    fn empty_storage_buffer(&self, size: u64, label: &'static str) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }
}

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn create_empty_rgba_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    channels: i32,
    label: &'static str,
) -> GpuImageTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    });
    GpuImageTexture {
        view: texture.create_view(&Default::default()),
        texture,
        spec: ImageSpec {
            width: width as i32,
            height: height as i32,
            channels,
            dtype: PixelType::F32,
        },
    }
}

fn texture_format(channels: i32) -> Result<wgpu::TextureFormat> {
    match channels {
        1 => Ok(wgpu::TextureFormat::R32Float),
        2 => Ok(wgpu::TextureFormat::Rg32Float),
        3 | 4 => Ok(wgpu::TextureFormat::Rgba32Float),
        _ => Err(eyre!("Unsupported channel count: {channels}")),
    }
}

fn normalized_rect(spec: &ImageSpec, rect: Recti) -> Recti {
    let bounds = Recti::from_min_size(Vec2i::new(0, 0), Vec2i::new(spec.width, spec.height));
    let rect = rect.validate();
    if rect.empty() {
        bounds
    } else {
        rect.intersect(bounds)
    }
}

fn reduction_group_count(params: &ComputeParams) -> u32 {
    let count = params.roi[2].saturating_mul(params.roi[3]);
    count.div_ceil(WORKGROUP_SIZE).clamp(1, REDUCTION_GROUPS)
}

pub(crate) fn rgb_rows_per_chunk(width: u32, max_binding_size: u64) -> u32 {
    let bytes_per_row = u64::from(width) * 3 * std::mem::size_of::<f32>() as u64;
    (max_binding_size / bytes_per_row).max(1).min(u64::from(u32::MAX)) as u32
}

fn ordered_to_float(value: u32) -> f32 {
    let bits = if value & 0x8000_0000 != 0 {
        value ^ 0x8000_0000
    } else {
        !value
    };
    f32::from_bits(bits)
}

fn ensure_compatible(lhs: &GpuImageTexture, rhs: &GpuImageTexture) -> Result<()> {
    if lhs.spec.width != rhs.spec.width || lhs.spec.height != rhs.spec.height || lhs.spec.channels != rhs.spec.channels
    {
        return Err(eyre!("Images must have matching dimensions and channels"));
    }
    Ok(())
}

const COMPUTE_SHADER: &str = r#"
struct Params {
    roi: vec4<u32>,
    image: vec4<u32>,
    operation: vec4<u32>,
    values: vec4<f32>,
}

@group(0) @binding(0) var source_a: texture_2d<f32>;
@group(0) @binding(1) var source_b: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<atomic<u32>>;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var destination: texture_storage_2d<rgba32float, write>;

var<workgroup> scratch: array<vec4<f32>, 256>;

fn finite(value: f32) -> bool {
    return value == value && abs(value) <= 3.402823466e+38;
}

fn float_to_ordered(value: f32) -> u32 {
    let bits = bitcast<u32>(value);
    if ((bits & 0x80000000u) != 0u) {
        return ~bits;
    }
    return bits ^ 0x80000000u;
}

fn roi_pixel(index: u32) -> vec2<i32> {
    let x = index % params.roi.z;
    let y = index / params.roi.z;
    return vec2<i32>(i32(params.roi.x + x), i32(params.roi.y + y));
}

fn channel_mask(value: vec4<f32>, channels: u32) -> vec4<f32> {
    return value * vec4<f32>(
        select(0.0, 1.0, channels > 0u),
        select(0.0, 1.0, channels > 1u),
        select(0.0, 1.0, channels > 2u),
        select(0.0, 1.0, channels > 3u));
}

@compute @workgroup_size(256)
fn minmax(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) workgroups: vec3<u32>) {
    let count = params.roi.z * params.roi.w;
    let stride = max(1u, workgroups.x * 256u);
    var index = gid.x;
    loop {
        if (index >= count) { break; }
        let value = textureLoad(source_a, roi_pixel(index), 0);
        for (var channel = 0u; channel < params.image.z; channel++) {
            let v = value[channel];
            if (finite(v)) {
                let ordered = float_to_ordered(v);
                atomicMin(&output[channel], ordered);
                atomicMax(&output[4u + channel], ordered);
            }
        }
        index += stride;
    }
}

@compute @workgroup_size(256)
fn histogram(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) workgroups: vec3<u32>) {
    let count = params.roi.z * params.roi.w;
    let stride = max(1u, workgroups.x * 256u);
    var index = gid.x;
    loop {
        if (index >= count) { break; }
        let value = textureLoad(source_a, roi_pixel(index), 0);
        for (var channel = 0u; channel < params.image.z; channel++) {
            let v = value[channel];
            if (v >= 0.0 && v < 1.0) {
                let bin = min(255u, u32(floor(v * 256.0)));
                atomicAdd(&output[channel * 256u + bin], 1u);
            }
        }
        index += stride;
    }
}

@compute @workgroup_size(256)
fn sum_values(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) workgroups: vec3<u32>) {
    let count = params.roi.z * params.roi.w;
    let stride = max(1u, workgroups.x * 256u);
    var sum = vec4<f32>(0.0);
    var index = gid.x;
    loop {
        if (index >= count) { break; }
        let a = channel_mask(textureLoad(source_a, roi_pixel(index), 0), params.image.z);
        if (params.operation.x == 0u) {
            sum += a;
        } else {
            let b = channel_mask(textureLoad(source_b, roi_pixel(index), 0), params.image.z);
            let d = a - b;
            sum += d * d;
        }
        index += stride;
    }
    scratch[lid.x] = sum;
    workgroupBarrier();
    var offset = 128u;
    loop {
        if (offset == 0u) { break; }
        if (lid.x < offset) { scratch[lid.x] += scratch[lid.x + offset]; }
        workgroupBarrier();
        offset /= 2u;
    }
    if (lid.x == 0u) {
        let base = wid.x * 4u;
        for (var channel = 0u; channel < 4u; channel++) {
            atomicStore(&output[base + channel], bitcast<u32>(scratch[0][channel]));
        }
    }
}

@compute @workgroup_size(256)
fn vector_mean(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>) {
    let output_count = select(params.roi.z, params.roi.w, params.operation.x == 1u);
    if (wid.x >= output_count) { return; }
    var sum = vec4<f32>(0.0);
    if (params.operation.x == 0u) {
        var row = lid.x;
        loop {
            if (row >= params.roi.w) { break; }
            sum += textureLoad(source_a, vec2<i32>(i32(params.roi.x + wid.x), i32(params.roi.y + row)), 0);
            row += 256u;
        }
    } else {
        var column = lid.x;
        loop {
            if (column >= params.roi.z) { break; }
            sum += textureLoad(source_a, vec2<i32>(i32(params.roi.x + column), i32(params.roi.y + wid.x)), 0);
            column += 256u;
        }
    }
    scratch[lid.x] = sum;
    workgroupBarrier();
    var offset = 128u;
    loop {
        if (offset == 0u) { break; }
        if (lid.x < offset) { scratch[lid.x] += scratch[lid.x + offset]; }
        workgroupBarrier();
        offset /= 2u;
    }
    if (lid.x == 0u) {
        let divisor = select(f32(params.roi.w), f32(params.roi.z), params.operation.x == 1u);
        let value = scratch[0] / divisor;
        let base = wid.x * 4u;
        for (var channel = 0u; channel < 4u; channel++) {
            atomicStore(&output[base + channel], bitcast<u32>(value[channel]));
        }
    }
}

fn normalize_channels(a: vec4<f32>, b: vec4<f32>) -> array<vec4<f32>, 2> {
    var result: array<vec4<f32>, 2>;
    result[0] = a;
    result[1] = b;
    switch params.operation.w {
        case 1u: { result[0] = vec4<f32>(a.x); }
        case 2u: { result[1] = vec4<f32>(b.x); }
        default: {}
    }
    return result;
}

@compute @workgroup_size(16, 16)
fn compare_images(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.image.x || gid.y >= params.image.y) { return; }
    let coord = vec2<i32>(gid.xy);
    let pair = normalize_channels(textureLoad(source_a, coord, 0), textureLoad(source_b, coord, 0));
    var value = pair[0] - pair[1];
    if (params.operation.x == 1u) {
        value = mix(pair[0], pair[1], params.values.x);
    }
    textureStore(destination, coord, value);
}

fn reflect101(value: i32, length: i32) -> i32 {
    if (length <= 1) { return 0; }
    var result = value;
    loop {
        if (result >= 0 && result < length) { break; }
        if (result < 0) { result = -result; }
        if (result >= length) { result = 2 * length - result - 2; }
    }
    return result;
}

fn gaussian_weight(offset: i32) -> f32 {
    let x = f32(offset);
    return exp(-(x * x) / 4.5);
}

fn ssim_pixel(index: u32) -> vec4<f32> {
    let local_x = i32(index % params.roi.z);
    let local_y = i32(index / params.roi.z);
    let width = i32(params.roi.z);
    let height = i32(params.roi.w);
    var mu_a = vec4<f32>(0.0);
    var mu_b = vec4<f32>(0.0);
    var aa = vec4<f32>(0.0);
    var bb = vec4<f32>(0.0);
    var ab = vec4<f32>(0.0);
    var weight_sum = 0.0;
    for (var oy = -5; oy <= 5; oy++) {
        let sy = reflect101(local_y + oy, height) + i32(params.roi.y);
        let wy = gaussian_weight(oy);
        for (var ox = -5; ox <= 5; ox++) {
            let sx = reflect101(local_x + ox, width) + i32(params.roi.x);
            let weight = wy * gaussian_weight(ox);
            let a = textureLoad(source_a, vec2<i32>(sx, sy), 0);
            let b = textureLoad(source_b, vec2<i32>(sx, sy), 0);
            mu_a += a * weight;
            mu_b += b * weight;
            aa += a * a * weight;
            bb += b * b * weight;
            ab += a * b * weight;
            weight_sum += weight;
        }
    }
    mu_a /= weight_sum;
    mu_b /= weight_sum;
    let variance_a = aa / weight_sum - mu_a * mu_a;
    let variance_b = bb / weight_sum - mu_b * mu_b;
    let covariance = ab / weight_sum - mu_a * mu_b;
    let numerator = (2.0 * mu_a * mu_b + vec4<f32>(0.0001)) * (2.0 * covariance + vec4<f32>(0.0009));
    let denominator = (mu_a * mu_a + mu_b * mu_b + vec4<f32>(0.0001))
        * (variance_a + variance_b + vec4<f32>(0.0009));
    return channel_mask(numerator / denominator, params.image.z);
}

@compute @workgroup_size(256)
fn ssim(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) workgroups: vec3<u32>) {
    let count = params.roi.z * params.roi.w;
    let stride = max(1u, workgroups.x * 256u);
    var sum = vec4<f32>(0.0);
    var index = gid.x;
    loop {
        if (index >= count) { break; }
        sum += ssim_pixel(index);
        index += stride;
    }
    scratch[lid.x] = sum;
    workgroupBarrier();
    var offset = 128u;
    loop {
        if (offset == 0u) { break; }
        if (lid.x < offset) { scratch[lid.x] += scratch[lid.x + offset]; }
        workgroupBarrier();
        offset /= 2u;
    }
    if (lid.x == 0u) {
        let base = wid.x * 4u;
        for (var channel = 0u; channel < 4u; channel++) {
            atomicStore(&output[base + channel], bitcast<u32>(scratch[0][channel]));
        }
    }
}
"#;

const RGB_UPLOAD_SHADER: &str = r#"
struct Params {
    base_y: u32,
    width: u32,
    height: u32,
    _padding: u32,
}

@group(0) @binding(0) var<storage, read> rgb: array<f32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.width || gid.y >= params.height) {
        return;
    }
    let index = (gid.y * params.width + gid.x) * 3u;
    textureStore(
        output,
        vec2<i32>(i32(gid.x), i32(params.base_y + gid.y)),
        vec4<f32>(rgb[index], rgb[index + 1u], rgb[index + 2u], 1.0));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Image, ImageData, PixelType};

    fn context() -> Arc<GpuComputeContext> {
        static CONTEXT: std::sync::OnceLock<Arc<GpuComputeContext>> = std::sync::OnceLock::new();
        Arc::clone(CONTEXT.get_or_init(|| {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            }))
            .expect("GPU adapter");
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("edolview compute test device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            }))
            .expect("GPU device");
            install_gpu_compute(&device, &queue, adapter.get_info().backend == wgpu::Backend::Dx12);
            gpu_compute().unwrap()
        }))
    }

    fn test_image(offset: f32) -> ImageData {
        let spec = ImageSpec::new(64, 32, 4, PixelType::F32);
        let pixels = (0..spec.width * spec.height)
            .flat_map(|index| {
                let value = index as f32 / (spec.width * spec.height) as f32 + offset;
                [value, value * 0.5, -value, 1.0]
            })
            .collect();
        ImageData::from_f32(spec, pixels).unwrap()
    }

    fn cpu_ssim(lhs: &ImageData, rhs: &ImageData) -> f64 {
        let spec = lhs.spec();
        let lhs = lhs.pixels().unwrap();
        let rhs = rhs.pixels().unwrap();
        let reflect = |mut value: i32, length: i32| {
            while value < 0 || value >= length {
                if value < 0 {
                    value = -value;
                }
                if value >= length {
                    value = 2 * length - value - 2;
                }
            }
            value
        };
        let mut total = 0.0f64;
        for y in 0..spec.height {
            for x in 0..spec.width {
                for channel in 0..spec.channels {
                    let mut weight_sum = 0.0f64;
                    let mut mean_a = 0.0f64;
                    let mut mean_b = 0.0f64;
                    let mut aa = 0.0f64;
                    let mut bb = 0.0f64;
                    let mut ab = 0.0f64;
                    for oy in -5..=5 {
                        let sy = reflect(y + oy, spec.height);
                        for ox in -5..=5 {
                            let sx = reflect(x + ox, spec.width);
                            let weight = (-(ox * ox + oy * oy) as f64 / 4.5).exp();
                            let index = ((sy * spec.width + sx) * spec.channels + channel) as usize;
                            let a = lhs[index] as f64;
                            let b = rhs[index] as f64;
                            weight_sum += weight;
                            mean_a += a * weight;
                            mean_b += b * weight;
                            aa += a * a * weight;
                            bb += b * b * weight;
                            ab += a * b * weight;
                        }
                    }
                    mean_a /= weight_sum;
                    mean_b /= weight_sum;
                    let variance_a = aa / weight_sum - mean_a * mean_a;
                    let variance_b = bb / weight_sum - mean_b * mean_b;
                    let covariance = ab / weight_sum - mean_a * mean_b;
                    total += ((2.0 * mean_a * mean_b + 0.0001) * (2.0 * covariance + 0.0009))
                        / ((mean_a * mean_a + mean_b * mean_b + 0.0001) * (variance_a + variance_b + 0.0009));
                }
            }
        }
        total / (spec.width * spec.height * spec.channels) as f64
    }

    #[test]
    fn gpu_statistics_match_expected_values() {
        let compute = context();
        let image = test_image(0.0);
        let texture = image.gpu_texture().unwrap();
        let (mins, maxs) = compute.minmax(&texture, Recti::ZERO).unwrap();
        assert!((mins[0] - 0.0).abs() < 1e-6);
        assert!((maxs[0] - 2047.0 / 2048.0).abs() < 1e-6);
        assert!((mins[2] + 2047.0 / 2048.0).abs() < 1e-6);
        assert_eq!(mins[3], 1.0);
        assert_eq!(maxs[3], 1.0);

        let mean = compute.mean(&texture, Recti::ZERO, MeanDim::All).unwrap();
        assert!((mean[0] - 2047.0 / 4096.0).abs() < 1e-5);
        assert!((mean[1] - 2047.0 / 8192.0).abs() < 1e-5);
        assert!((mean[2] + 2047.0 / 4096.0).abs() < 1e-5);
        assert!((mean[3] - 1.0).abs() < 1e-6);

        let columns = compute.mean(&texture, Recti::ZERO, MeanDim::Column).unwrap();
        let rows = compute.mean(&texture, Recti::ZERO, MeanDim::Row).unwrap();
        assert_eq!(columns.len(), 64 * 4);
        assert_eq!(rows.len(), 32 * 4);

        let histogram = compute.histogram(&texture).unwrap();
        assert_eq!(histogram.len(), 4);
        assert_eq!(histogram[0].iter().sum::<f32>(), 2048.0);
        assert_eq!(histogram[3].iter().sum::<f32>(), 0.0);
    }

    #[test]
    fn gpu_comparison_psnr_and_ssim_are_consistent() {
        let compute = context();
        let lhs = test_image(0.0);
        let rhs = test_image(0.01);
        let lhs_texture = lhs.gpu_texture().unwrap();
        let rhs_texture = rhs.gpu_texture().unwrap();
        let (psnr, rmse) = compute.psnr(&lhs_texture, &rhs_texture, Recti::ZERO, 1.0, 1.0).unwrap();
        assert!((rmse - 0.0075).abs() < 1e-5);
        assert!((psnr - 42.498775).abs() < 1e-3);

        let identical_ssim = compute.ssim(&lhs_texture, &lhs_texture, Recti::ZERO).unwrap();
        assert!((identical_ssim - 1.0).abs() < 1e-4);
        let expected_ssim = cpu_ssim(&lhs, &rhs);
        let actual_ssim = compute.ssim(&lhs_texture, &rhs_texture, Recti::ZERO).unwrap();
        assert!((actual_ssim - expected_ssim).abs() < 1e-4);

        let derived = ImageData::derived_comparison(lhs, rhs, lhs_texture.spec.clone(), ComparisonMode::Diff, 0.5, 0);
        let diff = derived.gpu_texture().unwrap();
        let (mins, maxs) = compute.minmax(&diff, Recti::ZERO).unwrap();
        for (channel, expected) in [-0.01, -0.005, 0.01, 0.0].into_iter().enumerate() {
            assert!((mins[channel] - expected).abs() < 1e-5);
            assert!((maxs[channel] - expected).abs() < 1e-5);
        }
    }
}
