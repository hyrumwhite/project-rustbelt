//! An NRC context on a wgpu device: its buffers and the per-frame calls.

use std::{ffi::c_void, ptr, sync::Arc};

use ash::vk::{self, Handle};
use wgpu::{CommandEncoder, Device, Queue, hal::api::Vulkan};

use crate::{
    AllocationInfo, BufferIdx, ContextSettings, FrameSettings, Nrc, NrcConstants, NrcError, Status, library, sys,
};

/// One NRC buffer: a wgpu buffer over memory this crate allocated.
#[derive(Debug)]
pub struct NrcBuffer {
    /// The buffer, for bind groups (usage `STORAGE | COPY_SRC | COPY_DST`).
    pub buffer: wgpu::Buffer,
    /// What the library asked for.
    pub allocation: AllocationInfo,
    /// Its `VkDeviceAddress`.
    pub device_address: u64,
    /// Its `VkDeviceMemory` (owned by wgpu with the buffer), for the library.
    memory: u64,
}

/// All of a context's buffers, by [`BufferIdx`]; a buffer the settings do not
/// need (size 0) is `None`.
#[derive(Debug)]
pub struct NrcBuffers {
    buffers: [Option<NrcBuffer>; sys::BUFFER_COUNT],
    /// Handed to the library in place of the buffers the settings do not
    /// need: it writes a descriptor for every buffer (e.g. the debug one with
    /// debug buffers off), and a null VkBuffer there is invalid.
    placeholder: Option<NrcBuffer>,
}

impl NrcBuffers {
    /// The buffer at `index`, if the settings need one.
    pub fn get(&self, index: BufferIdx) -> Option<&NrcBuffer> {
        self.buffers[index as usize].as_ref()
    }

    /// The total bytes allocated.
    pub fn total_size(&self) -> u64 {
        self.buffers.iter().flatten().map(|b| b.buffer.size()).sum()
    }
}

/// An NRC context (`nrc::vulkan::Context`) on a wgpu Vulkan device.
///
/// Per frame: [`begin_frame`](Self::begin_frame), the path tracer's passes
/// (writing query and training records into the buffers),
/// [`query_and_train`](Self::query_and_train), submit, then
/// [`end_frame`](Self::end_frame).
///
/// The library records into raw Vulkan command buffers, and wgpu does not
/// allow raw commands and wgpu commands in one [`CommandEncoder`]: the
/// encoder given to `begin_frame` and `query_and_train` must be used for
/// nothing else (wgpu panics on mixing). Submit it in order between the
/// encoders of the passes around it. Each call is bracketed by full memory
/// barriers, since wgpu cannot see what the library reads and writes.
pub struct NrcContext {
    raw: *mut c_void,
    device: Device,
    vk_device: ash::Device,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    settings: Option<ContextSettings>,
    allocation: Option<sys::BuffersAllocationInfo>,
    buffers: Option<NrcBuffers>,
    /// Where QueryAndTrain writes the loss when asked (boxed: the library may
    /// write it after the call returns).
    training_loss: Box<f32>,
    // (dropped last: Shutdown after Destroy)
    _nrc: Arc<Nrc>,
}

// SAFETY: the context pointer is only used through &mut self (or &self for
// read-only queries), so the library never sees concurrent calls on one context.
unsafe impl Send for NrcContext {}
// SAFETY: the &self methods only read (PopulateShaderConstants is const in C++).
unsafe impl Sync for NrcContext {}

impl NrcContext {
    /// Creates a context on `device`, which must be Vulkan and created with
    /// NRC's extensions ([`crate::request_device`] or
    /// [`crate::register_device_extensions`]).
    pub fn new(nrc: &Arc<Nrc>, device: &Device) -> Result<Self, NrcError> {
        library::ensure_loaded()?;
        // SAFETY: the hal device is only used for its handles, while `device` (cloned into self) lives.
        let hal_device = unsafe { device.as_hal::<Vulkan>() }.ok_or(NrcError::NotVulkan)?;
        let vk_device = hal_device.raw_device().clone();
        let physical = hal_device.raw_physical_device();
        let instance = hal_device.shared_instance().raw_instance();
        // SAFETY: a query on the device's own physical device.
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical) };
        let mut raw = ptr::null_mut();
        // SAFETY: valid handles of a live device; the library writes the context pointer.
        let status = Status::from_raw(unsafe {
            sys::nrc_shim_context_create(
                vk_device.handle().as_raw(),
                physical.as_raw(),
                instance.handle().as_raw(),
                &mut raw,
            )
        });
        if status != Status::Ok || raw.is_null() {
            return Err(NrcError::Status(if status == Status::Ok { Status::InternalError } else { status }));
        }
        drop(hal_device);
        Ok(Self {
            raw,
            device: device.clone(),
            vk_device,
            memory_properties,
            settings: None,
            allocation: None,
            buffers: None,
            training_loss: Box::new(0.0),
            _nrc: Arc::clone(nrc),
        })
    }

    /// What the library needs allocated for `settings`, by [`BufferIdx`].
    pub fn allocation_info(settings: &ContextSettings) -> Result<[AllocationInfo; 9], NrcError> {
        library::ensure_loaded()?;
        Ok(raw_allocation_info(settings)?.allocation_info)
    }

    /// (Re)configures the context: allocates the buffers for `settings` when
    /// their sizes change, then calls `Configure`. It can hitch (allocation,
    /// a wait for the GPU when buffers are replaced); call it when the
    /// settings change, not every frame.
    pub fn configure(&mut self, settings: &ContextSettings) -> Result<(), NrcError> {
        let allocation = raw_allocation_info(settings)?;
        let same_sizes = self.allocation.is_some_and(|old| {
            old.allocation_info.iter().zip(&allocation.allocation_info).all(|(a, b)| {
                a.size() == b.size() && a.use_readback_heap == b.use_readback_heap
            })
        });
        if !same_sizes {
            // (the library may still be using the old buffers on the GPU)
            if self.buffers.is_some() {
                // SAFETY: waiting on our own device.
                unsafe { self.vk_device.device_wait_idle() }.map_err(NrcError::Vulkan)?;
            }
            self.buffers = None;
            self.buffers = Some(self.create_buffers(&allocation)?);
            self.allocation = Some(allocation);
        }
        let buffers = self.buffers.as_ref().unwrap();
        let mut raw_buffers = sys::Buffers::default();
        for (info, buffer) in raw_buffers.buffers.iter_mut().zip(&buffers.buffers) {
            if let Some(buffer) = buffer.as_ref().or(buffers.placeholder.as_ref()) {
                // SAFETY: the hal buffer handle is only read; the wgpu buffer outlives its use.
                let hal = unsafe { buffer.buffer.as_hal::<Vulkan>() }.ok_or(NrcError::NotVulkan)?;
                // SAFETY: reading the handle; it stays owned by wgpu.
                info.resource = unsafe { hal.raw_handle() }.as_raw();
                info.allocated_size = buffer.buffer.size() as usize;
                info.allocated_offset = 0;
                info.memory = buffer.memory;
                info.device_address = buffer.device_address;
            }
        }
        // SAFETY: a live context; `settings` and `raw_buffers` match the C++
        // structs and the buffers live in self until the next configure or drop.
        let status = Status::from_raw(unsafe {
            sys::nrc_shim_context_configure(self.raw, settings, &raw_buffers, ptr::null())
        });
        if status != Status::Ok {
            return Err(NrcError::Status(status));
        }
        self.settings = Some(*settings);
        Ok(())
    }

    /// The buffers, once configured.
    pub fn buffers(&self) -> Option<&NrcBuffers> {
        self.buffers.as_ref()
    }

    /// The settings last configured.
    pub fn settings(&self) -> Option<&ContextSettings> {
        self.settings.as_ref()
    }

    /// The constants the shaders need (`PopulateShaderConstants`), for a uniform buffer.
    pub fn shader_constants(&self) -> Result<NrcConstants, NrcError> {
        let mut constants = NrcConstants::default();
        // SAFETY: a live context; `constants` matches NrcConstants (96 bytes).
        check(unsafe { sys::nrc_shim_context_populate_shader_constants(self.raw, &mut constants) })?;
        Ok(constants)
    }

    /// Records `BeginFrame` (the library's per-frame housekeeping: clears the
    /// counters) into `encoder`, which must hold only raw commands (see the
    /// type's docs). Call before the passes that write records.
    pub fn begin_frame(&mut self, encoder: &mut CommandEncoder, frame_settings: &FrameSettings) -> Result<(), NrcError> {
        self.require_configured()?;
        let raw = self.raw;
        self.record(encoder, |command_buffer| {
            // SAFETY: a live, configured context; the command buffer is recording.
            unsafe { sys::nrc_shim_context_begin_frame(raw, command_buffer.as_raw(), frame_settings) }
        })
    }

    /// Records `QueryAndTrain` (inference on the frame's queries, then
    /// training) into `encoder` (raw commands only, see the type's docs).
    /// With `report_loss`, the library writes the training loss, read with
    /// [`training_loss`](Self::training_loss) (NVIDIA: costs performance).
    pub fn query_and_train(&mut self, encoder: &mut CommandEncoder, report_loss: bool) -> Result<(), NrcError> {
        self.require_configured()?;
        let raw = self.raw;
        let loss: *mut f32 = if report_loss { &mut *self.training_loss } else { ptr::null_mut() };
        self.record(encoder, |command_buffer| {
            // SAFETY: a live, configured context; the command buffer is
            // recording; `loss` is null or points into a Box owned by self.
            unsafe { sys::nrc_shim_context_query_and_train(raw, command_buffer.as_raw(), loss) }
        })
    }

    /// The last training loss the library reported (see `query_and_train`).
    pub fn training_loss(&self) -> f32 {
        *self.training_loss
    }

    /// `EndFrame`: call after the frame's command buffers were submitted to
    /// `queue`, the queue all NRC work goes to. Not concurrently with other
    /// submissions on that queue (Vulkan queues need external sync).
    pub fn end_frame(&mut self, queue: &Queue) -> Result<(), NrcError> {
        self.require_configured()?;
        // SAFETY: the hal queue is only read for its handle while `queue` lives.
        let raw_queue = unsafe { queue.as_hal::<Vulkan>() }.ok_or(NrcError::NotVulkan)?.as_raw();
        // SAFETY: a live, configured context and the device's queue.
        check(unsafe { sys::nrc_shim_context_end_frame(self.raw, raw_queue.as_raw()) })
    }

    fn require_configured(&self) -> Result<(), NrcError> {
        if self.settings.is_none() { Err(NrcError::NotConfigured) } else { Ok(()) }
    }

    /// Runs `call` on the encoder's raw command buffer between two full memory barriers.
    fn record(&self, encoder: &mut CommandEncoder, call: impl FnOnce(vk::CommandBuffer) -> i32) -> Result<(), NrcError> {
        let device = &self.vk_device;
        // SAFETY: the raw command buffer is recording while the closure runs
        // (wgpu opened it); the barriers and the library's commands are valid
        // Vulkan commands for it. wgpu panics if the encoder was also used
        // through its own API, which the docs forbid.
        let status = unsafe {
            encoder.as_hal_mut::<Vulkan, _, _>(|hal| {
                let Some(hal) = hal else { return Err(NrcError::EncoderInvalid) };
                let command_buffer = hal.raw_handle();
                full_barrier(device, command_buffer);
                let status = call(command_buffer);
                full_barrier(device, command_buffer);
                Ok(status)
            })
        }?;
        check(status)
    }

    fn create_buffers(&self, allocation: &sys::BuffersAllocationInfo) -> Result<NrcBuffers, NrcError> {
        let mut buffers: [Option<NrcBuffer>; sys::BUFFER_COUNT] = Default::default();
        let mut placeholder = None;
        for (index, info) in allocation.allocation_info.iter().enumerate() {
            if info.size() == 0 {
                if placeholder.is_none() {
                    let info = AllocationInfo { element_count: 64, element_size: 4, ..*info };
                    placeholder = Some(self.create_buffer(BufferIdx::ALL[index], &info)?);
                }
                continue;
            }
            buffers[index] = Some(self.create_buffer(BufferIdx::ALL[index], info)?);
        }
        Ok(NrcBuffers { buffers, placeholder })
    }

    /// A buffer with the usage the library asks for plus storage, transfer
    /// and device address, in memory with `VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT`
    /// (host-visible for the readback ones), handed to wgpu, which frees both.
    fn create_buffer(&self, index: BufferIdx, info: &AllocationInfo) -> Result<NrcBuffer, NrcError> {
        let device = &self.vk_device;
        let size = info.size().next_multiple_of(4);
        // SAFETY: a live context; `info` matches nrc::AllocationInfo.
        let wanted = unsafe { sys::nrc_shim_context_get_buffer_usage_flags(self.raw, info) };
        let usage = vk::BufferUsageFlags::from_raw(wanted)
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS;
        let create_info = vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: a valid create info on our device.
        let raw_buffer = unsafe { device.create_buffer(&create_info, None) }.map_err(NrcError::Vulkan)?;
        // SAFETY: the buffer was just created on this device.
        let requirements = unsafe { device.get_buffer_memory_requirements(raw_buffer) };
        let properties = if info.use_readback_heap {
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        } else {
            vk::MemoryPropertyFlags::DEVICE_LOCAL
        };
        let Some(memory_type) = self.memory_type(requirements.memory_type_bits, properties) else {
            // SAFETY: not bound or used yet.
            unsafe { device.destroy_buffer(raw_buffer, None) };
            return Err(NrcError::NoMemoryType(index));
        };
        let mut flags = vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
        let allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type)
            .push_next(&mut flags);
        // SAFETY: a valid allocate info; the buffer is bound to it at offset 0
        // right after, and both are released on failure.
        let (memory, device_address) = unsafe {
            let memory = match device.allocate_memory(&allocate_info, None) {
                Ok(memory) => memory,
                Err(e) => {
                    device.destroy_buffer(raw_buffer, None);
                    return Err(NrcError::Vulkan(e));
                }
            };
            if let Err(e) = device.bind_buffer_memory(raw_buffer, memory, 0) {
                device.destroy_buffer(raw_buffer, None);
                device.free_memory(memory, None);
                return Err(NrcError::Vulkan(e));
            }
            let address = device.get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(raw_buffer));
            (memory, address)
        };
        if device_address == 0 {
            return Err(NrcError::NoDeviceAddress(index));
        }
        let label = format!("nrc {}", info.debug_name());
        // SAFETY: wgpu takes ownership of the buffer and its memory (it
        // destroys and frees them when the wgpu buffer is destroyed); nothing
        // else frees them.
        let buffer = unsafe {
            let hal = wgpu::hal::vulkan::Buffer::from_raw_managed(raw_buffer, memory, 0, requirements.size);
            self.device.create_buffer_from_hal::<Vulkan>(
                hal,
                &wgpu::BufferDescriptor {
                    label: Some(&label),
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                },
            )
        };
        log::debug!(target: "nrc", "{label}: {size} bytes, usage {usage:?}, address {device_address:#x}, memory {:#x}", memory.as_raw());
        Ok(NrcBuffer { buffer, allocation: *info, device_address, memory: memory.as_raw() })
    }

    fn memory_type(&self, type_bits: u32, properties: vk::MemoryPropertyFlags) -> Option<u32> {
        let types = &self.memory_properties.memory_types[..self.memory_properties.memory_type_count as usize];
        let fits = |i: usize, flags: vk::MemoryPropertyFlags| type_bits & (1 << i) != 0 && types[i].property_flags.contains(flags);
        // (cached host memory first for readback)
        let preferred = if properties.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
            properties | vk::MemoryPropertyFlags::HOST_CACHED
        } else {
            properties
        };
        (0..types.len()).find(|&i| fits(i, preferred)).or_else(|| (0..types.len()).find(|&i| fits(i, properties))).map(|i| i as u32)
    }
}

impl Drop for NrcContext {
    fn drop(&mut self) {
        // SAFETY: wait for the library's GPU work, then destroy the context
        // before its buffers (dropped with self afterwards) and before
        // Shutdown (the Arc<Nrc> drops last).
        unsafe {
            let _ = self.vk_device.device_wait_idle();
            let status = Status::from_raw(sys::nrc_shim_context_destroy(self.raw));
            if status != Status::Ok {
                log::error!(target: "nrc", "Context::Destroy: {status:?}");
            }
        }
        self.buffers = None;
    }
}

fn raw_allocation_info(settings: &ContextSettings) -> Result<sys::BuffersAllocationInfo, NrcError> {
    // SAFETY: zero is a valid AllocationInfo (null name); the library fills all nine.
    let mut info: sys::BuffersAllocationInfo = unsafe { std::mem::zeroed() };
    // SAFETY: the structs match the C++ ones; GetBuffersAllocationInfo is static.
    check(unsafe { sys::nrc_shim_get_buffers_allocation_info(settings, &mut info) })?;
    Ok(info)
}

fn check(status: i32) -> Result<(), NrcError> {
    match Status::from_raw(status) {
        Status::Ok => Ok(()),
        status => Err(NrcError::Status(status)),
    }
}

/// A full memory barrier: all earlier commands' writes visible to all later commands.
///
/// # Safety
/// `command_buffer` must be recording on `device`.
unsafe fn full_barrier(device: &ash::Device, command_buffer: vk::CommandBuffer) {
    let barrier = vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
        .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE);
    // SAFETY: the caller guarantees the command buffer is recording.
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &[barrier],
            &[],
            &[],
        )
    };
}
