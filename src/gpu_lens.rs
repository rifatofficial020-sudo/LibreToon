//! Compute pass for the full-quality lens kernel, with CPU fallback.
use eframe::wgpu::{self, util::DeviceExt};
use std::sync::{Arc, OnceLock};

thread_local! {
    static WORKER_GPU: std::cell::RefCell<Option<Arc<GpuLens>>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn use_on_worker(gpu: Option<Arc<GpuLens>>) {
    WORKER_GPU.with(|slot| *slot.borrow_mut() = gpu);
}

pub(crate) fn blur_on_worker(
    image: &image::RgbaImage,
    offsets: &[(i32, i32)],
) -> Option<image::RgbaImage> {
    if (image.len() / 4).saturating_mul(offsets.len()) < 8_000_000 {
        return None;
    }
    WORKER_GPU.with(|slot| slot.borrow().as_ref()?.blur(image, offsets))
}

pub(crate) struct GpuLens {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: OnceLock<Option<wgpu::ComputePipeline>>,
}

impl GpuLens {
    pub(crate) fn from_renderer(render: &eframe::egui_wgpu::RenderState) -> Option<Self> {
        let caps = render.adapter.get_downlevel_capabilities();
        let limits = render.device.limits();
        (caps.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
            && render.adapter.get_info().device_type != wgpu::DeviceType::Cpu
            && limits.max_compute_invocations_per_workgroup >= 256
            && limits.max_compute_workgroup_size_x >= 16
            && limits.max_compute_workgroup_size_y >= 16
            && limits.max_storage_buffers_per_shader_stage >= 3)
            .then(|| Self::new(render.device.clone(), render.queue.clone()))
    }

    fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            device,
            queue,
            pipeline: OnceLock::new(),
        }
    }

    fn create_pipeline(&self) -> Option<wgpu::ComputePipeline> {
        let device = &self.device;
        let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Lens blur parity probe"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
    @group(0) @binding(0) var<storage, read> src: array<u32>;
    @group(0) @binding(1) var<storage, read> kernel: array<i32>;
    @group(0) @binding(2) var<storage, read_write> dst: array<u32>;

    @compute @workgroup_size(16, 16)
    fn main(@builtin(global_invocation_id) pos: vec3<u32>) {
        let w = kernel[0];
        let h = kernel[1];
        let n = kernel[2];
        if (pos.x >= u32(w) || pos.y >= u32(h)) { return; }
        let shifts = vec4<u32>(0, 8, 16, 24);
        var sum = vec4<u32>(0);
        for (var i = 0; i < n; i++) {
            let x = i32(pos.x) + kernel[3 + 2 * i];
            let y = i32(pos.y) + kernel[4 + 2 * i];
            if (x >= 0 && x < w && y >= 0 && y < h) {
                sum += (vec4<u32>(src[y * w + x]) >> shifts) & vec4<u32>(255);
            }
        }
        let packed = (sum / vec4<u32>(u32(n))) << shifts;
        dst[pos.y * u32(w) + pos.x] = packed.x | packed.y | packed.z | packed.w;
    }
    "#
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Lens blur parity probe"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if block_on(errors.pop()).is_some() {
            None
        } else {
            Some(pipeline)
        }
    }

    fn blur(&self, source: &image::RgbaImage, offsets: &[(i32, i32)]) -> Option<image::RgbaImage> {
        let (w, h) = source.dimensions();
        let device = &self.device;
        let limits = device.limits();
        if w == 0
            || h == 0
            || offsets.is_empty()
            || source.len() as u64 > limits.max_storage_buffer_binding_size
            || source.len() as u64 > limits.max_buffer_size
            || w.div_ceil(16).max(h.div_ceil(16)) > limits.max_compute_workgroups_per_dimension
        {
            return None;
        }
        let pipeline = self
            .pipeline
            .get_or_init(|| self.create_pipeline())
            .as_ref()?;
        let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = self.dispatch(source, offsets, pipeline);
        let invalid = block_on(validation.pop()).is_some();
        let exhausted = block_on(allocation.pop()).is_some();
        if invalid || exhausted { None } else { result }
    }

    fn dispatch(
        &self,
        source: &image::RgbaImage,
        offsets: &[(i32, i32)],
        pipeline: &wgpu::ComputePipeline,
    ) -> Option<image::RgbaImage> {
        let (w, h) = source.dimensions();
        let device = &self.device;
        let queue = &self.queue;
        let mut kernel = vec![w as i32, h as i32, offsets.len() as i32];
        kernel.extend(offsets.iter().flat_map(|&(x, y)| [x, y]));
        let kernel: Vec<_> = kernel.into_iter().flat_map(i32::to_le_bytes).collect();
        let buffer = |data| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: data,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let src = buffer(source.as_raw());
        let kernel = buffer(&kernel);
        let size = u64::from(w) * u64::from(h) * 4;
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = pipeline.get_bind_group_layout(0);
        let entries = [&src, &kernel, &dst];
        let entries: Vec<_> = entries
            .into_iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &entries,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&dst, 0, &readback, 0, size);
        let submitted = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submitted),
                timeout: Some(std::time::Duration::from_secs(2)),
            })
            .ok()?;
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .ok()?
            .ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        image::RgbaImage::from_raw(w, h, mapped.to_vec())
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
#[ignore = "manual compute benchmark requiring a compatible GPU adapter"]
fn benchmark_gpu_lens_parity() {
    let instance = wgpu::Instance::default();
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter unavailable");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .expect("GPU device unavailable");
    let lens = GpuLens::new(device, queue);
    assert!(
        lens.pipeline
            .get_or_init(|| lens.create_pipeline())
            .is_some()
    );
    let (w, h) = (1920u32, 1080u32);
    let source = image::RgbaImage::from_fn(w, h, |x, y| {
        let a = ((x + 3 * y) % 256) as u8;
        image::Rgba([a / 2, a / 3, a / 4, a])
    });
    let offsets: Vec<_> = (0..48)
        .map(|i| {
            let a = (i as f32) * 2.399963;
            let r = ((i as f32 + 0.5) / 48.).sqrt() * 42.;
            ((a.cos() * r).round() as i32, (a.sin() * r).round() as i32)
        })
        .collect();
    let start = std::time::Instant::now();
    let reference = crate::depth::lens_blur(&source, &offsets);
    let cpu = start.elapsed();
    let start = std::time::Instant::now();
    let pixels = lens.blur(&source, &offsets).expect("GPU lens pass");
    let gpu = start.elapsed();
    assert!(pixels == reference);
    eprintln!(
        "{}: CPU {cpu:?}; GPU with transfers {gpu:?}; exact RGBA match",
        adapter.get_info().name
    );
}
