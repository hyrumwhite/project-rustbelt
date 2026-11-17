//! The overlay's shared textures (protocol 56, docs/overlay-sharing.md):
//! Vulkan images with exportable memory, one allocation each, whose handles
//! are duplicated into the game's process for it to import into GL and copy
//! its picture into, so the picture crosses on the GPU in place of the read
//! back, the shared memory and the upload. Windows only (opaque Win32
//! handles); everything here is the Vulkan and handle side, the bookkeeping
//! is `super`'s.

use ash::vk;
use halo_bridge::{OverlaySet, OVERLAY_HANDLE_OPAQUE_WIN32, OVERLAY_SHARED_SLOTS};
use wgpu::hal::api::Vulkan;

/// The images' format: the overlay's bytes are display-encoded, as the
/// pixel path's `Rgba8UnormSrgb` texture takes them.
const FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
const HANDLE_TYPE: vk::ExternalMemoryHandleTypeFlags = vk::ExternalMemoryHandleTypeFlags::OPAQUE_WIN32;
/// MUTABLE_FORMAT: EXT_external_objects (issue 17) has the Vulkan side set
/// it for an image a GL 4.3 or later texture aliases, since GL may view the
/// texture in another format of its class.
const FLAGS: vk::ImageCreateFlags = vk::ImageCreateFlags::MUTABLE_FORMAT;

/// What the device can do, found once.
#[derive(Debug, Clone, Copy)]
pub struct Capability {
    /// The image usage: every usage GL may put a colour texture to
    /// (EXT_external_objects, issue 13: all supported usages), as far as
    /// the format allows them.
    usage: vk::ImageUsageFlags,
}

unsafe extern "system" {
    fn GetCurrentProcess() -> isize;
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> isize;
    fn DuplicateHandle(source_process: isize, source: isize, target_process: isize, target: *mut isize, access: u32, inherit: i32, options: u32) -> i32;
    fn CloseHandle(handle: isize) -> i32;
    fn GetLastError() -> u32;
}
const PROCESS_DUP_HANDLE: u32 = 0x0040;
const DUPLICATE_CLOSE_SOURCE: u32 = 0x1;
const DUPLICATE_SAME_ACCESS: u32 = 0x2;

/// Whether the device can make exportable images the game can import, and
/// with what usage; the reason if not.
pub fn capability(device: &wgpu::Device) -> Result<Capability, String> {
    // SAFETY: the hal device is only used for its handles while `device` lives
    let hal = unsafe { device.as_hal::<Vulkan>() }.ok_or("the device is not Vulkan")?;
    if !hal.enabled_device_extensions().contains(&ash::khr::external_memory_win32::NAME) {
        return Err("VK_KHR_external_memory_win32 is not enabled".into());
    }
    let instance = hal.shared_instance();
    if instance.instance_api_version() < vk::API_VERSION_1_1 {
        return Err("Vulkan 1.1 is needed".into());
    }
    let raw = instance.raw_instance();
    let physical = hal.raw_physical_device();
    // SAFETY: a query on the device's own physical device
    let features = unsafe { raw.get_physical_device_format_properties(physical, FORMAT) }.optimal_tiling_features;
    let mut usage = vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED;
    for (feature, flag) in [
        (vk::FormatFeatureFlags::COLOR_ATTACHMENT, vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::INPUT_ATTACHMENT),
        (vk::FormatFeatureFlags::STORAGE_IMAGE, vk::ImageUsageFlags::STORAGE),
    ] {
        if features.contains(feature) {
            usage |= flag;
        }
    }
    let mut external_info = vk::PhysicalDeviceExternalImageFormatInfo::default().handle_type(HANDLE_TYPE);
    let info = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(FORMAT)
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .flags(FLAGS)
        .push_next(&mut external_info);
    let mut external = vk::ExternalImageFormatProperties::default();
    let mut properties = vk::ImageFormatProperties2::default().push_next(&mut external);
    // SAFETY: as above; the structures are chained as the query wants them
    unsafe { raw.get_physical_device_image_format_properties2(physical, &info, &mut properties) }.map_err(|e| format!("the format query failed ({e})"))?;
    let features = external.external_memory_properties.external_memory_features;
    if !features.contains(vk::ExternalMemoryFeatureFlags::EXPORTABLE) {
        return Err(format!("R8G8B8A8_SRGB images cannot be exported as opaque Win32 handles (features {features:?})"));
    }
    Ok(Capability { usage })
}

/// A set of shared textures, as wgpu holds them (wgpu frees each image and
/// its memory once the last use of it on the GPU is done).
pub struct Slots {
    pub textures: Vec<wgpu::Texture>,
}

/// One image with its own exportable, dedicated allocation, and the
/// allocation's handle in this process.
struct Made {
    image: vk::Image,
    memory: vk::DeviceMemory,
    allocation_size: u64,
    handle: isize,
}

/// Makes a set of `OVERLAY_SHARED_SLOTS` textures of this size and
/// duplicates their handles into the game's process (`process_id`): the
/// set, and the offer to write into the bridge.
pub fn make(device: &wgpu::Device, capability: &Capability, serial: u32, size: (u32, u32), process_id: u32) -> Result<(Slots, OverlaySet), String> {
    // SAFETY: as in capability()
    let hal = unsafe { device.as_hal::<Vulkan>() }.ok_or("the device is not Vulkan")?;
    let raw = hal.raw_device();
    let instance = hal.shared_instance().raw_instance();
    let memory_win32 = ash::khr::external_memory_win32::Device::new(instance, raw);
    let physical = hal.raw_physical_device();
    // SAFETY: a query on the device's own physical device
    let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical) };

    // (on any failure the images and allocations made so far are freed and
    // their handles closed)
    let mut made: Vec<Made> = Vec::with_capacity(OVERLAY_SHARED_SLOTS);
    let free = |made: &mut Vec<Made>| {
        for m in made.drain(..) {
            // SAFETY: made here, never handed to wgpu or the game
            unsafe {
                if m.handle != 0 {
                    CloseHandle(m.handle);
                }
                raw.destroy_image(m.image, None);
                raw.free_memory(m.memory, None);
            }
        }
    };
    for _ in 0..OVERLAY_SHARED_SLOTS {
        match make_one(raw, &memory_win32, &memory_properties, capability, size) {
            Ok(m) => made.push(m),
            Err(e) => {
                free(&mut made);
                return Err(e);
            }
        }
    }

    // the handles into the game's process
    // SAFETY: plain Win32 calls; every handle opened is closed
    let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, process_id) };
    if process == 0 {
        let error = unsafe { GetLastError() };
        free(&mut made);
        return Err(format!("OpenProcess({process_id}) failed (error {error})"));
    }
    let mut handles = [0u32; OVERLAY_SHARED_SLOTS];
    for (index, m) in made.iter().enumerate() {
        let mut target = 0isize;
        // SAFETY: as above
        let ok = unsafe { DuplicateHandle(GetCurrentProcess(), m.handle, process, &mut target, 0, 0, DUPLICATE_SAME_ACCESS) } != 0;
        if !ok || target == 0 || target as u64 > u32::MAX as u64 {
            let error = unsafe { GetLastError() };
            // (those already in the game's process closed there)
            for &handle in &handles[..index] {
                // SAFETY: as above
                unsafe { DuplicateHandle(process, handle as isize, 0, std::ptr::null_mut(), 0, 0, DUPLICATE_CLOSE_SOURCE) };
            }
            if ok && target != 0 {
                // SAFETY: as above
                unsafe { DuplicateHandle(process, target, 0, std::ptr::null_mut(), 0, 0, DUPLICATE_CLOSE_SOURCE) };
            }
            unsafe { CloseHandle(process) };
            free(&mut made);
            return Err(format!("DuplicateHandle failed (error {error}, handle {target:#x})"));
        }
        handles[index] = target as u32;
    }
    // SAFETY: as above; the game holds its own handles now
    unsafe { CloseHandle(process) };
    let allocation_size = made[0].allocation_size;

    // wgpu takes the images and their memory
    let label = format!("game overlay shared {serial}");
    let hal_descriptor = wgpu::hal::TextureDescriptor {
        label: Some(&label),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUses::RESOURCE,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: vec![],
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some(&label),
        size: hal_descriptor.size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let textures = made
        .drain(..)
        .map(|m| {
            // SAFETY: our own handle, no longer needed (the game's are its own)
            unsafe { CloseHandle(m.handle) };
            // SAFETY: the image was made to the descriptor (the usage a
            // superset), bound to its memory, which wgpu-hal now owns; it is
            // uninitialized (layout UNDEFINED) until the game copies into it
            unsafe {
                let texture = hal.texture_from_raw(m.image, &hal_descriptor, None, wgpu::hal::vulkan::TextureMemory::Dedicated(m.memory));
                device.create_texture_from_hal::<Vulkan>(texture, &descriptor, wgpu::TextureUses::UNINITIALIZED)
            }
        })
        .collect();
    let set = OverlaySet {
        serial,
        process_id,
        width: size.0,
        height: size.1,
        allocation_size,
        dedicated: true,
        handle_type: OVERLAY_HANDLE_OPAQUE_WIN32,
        handles,
    };
    Ok((Slots { textures }, set))
}

fn make_one(
    raw: &ash::Device,
    memory_win32: &ash::khr::external_memory_win32::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    capability: &Capability,
    size: (u32, u32),
) -> Result<Made, String> {
    let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(HANDLE_TYPE);
    let info = vk::ImageCreateInfo::default()
        .flags(FLAGS)
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D { width: size.0, height: size.1, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(capability.usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut external);
    // SAFETY: a valid device; the image is destroyed on every failure below
    let image = unsafe { raw.create_image(&info, None) }.map_err(|e| format!("vkCreateImage failed ({e})"))?;
    let requirements = unsafe { raw.get_image_memory_requirements(image) };
    let Some(type_index) = (0..memory_properties.memory_type_count).find(|&i| {
        requirements.memory_type_bits & (1 << i) != 0
            && memory_properties.memory_types[i as usize].property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
    }) else {
        unsafe { raw.destroy_image(image, None) };
        return Err("no device-local memory type for the image".into());
    };
    // a dedicated allocation always (some drivers require one for exported
    // images, and GL is then told so)
    let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
    let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(HANDLE_TYPE);
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(type_index)
        .push_next(&mut export)
        .push_next(&mut dedicated);
    // SAFETY: as above
    let memory = match unsafe { raw.allocate_memory(&allocate, None) } {
        Ok(memory) => memory,
        Err(e) => {
            unsafe { raw.destroy_image(image, None) };
            return Err(format!("vkAllocateMemory failed ({e})"));
        }
    };
    let fail = |message: String| {
        // SAFETY: made above, not handed on
        unsafe {
            raw.destroy_image(image, None);
            raw.free_memory(memory, None);
        }
        Err(message)
    };
    if let Err(e) = unsafe { raw.bind_image_memory(image, memory, 0) } {
        return fail(format!("vkBindImageMemory failed ({e})"));
    }
    let get = vk::MemoryGetWin32HandleInfoKHR::default().memory(memory).handle_type(HANDLE_TYPE);
    // SAFETY: memory allocated exportable as this handle type
    let handle = match unsafe { memory_win32.get_memory_win32_handle(&get) } {
        Ok(handle) if handle != 0 => handle,
        Ok(_) => return fail("vkGetMemoryWin32HandleKHR gave no handle".into()),
        Err(e) => return fail(format!("vkGetMemoryWin32HandleKHR failed ({e})")),
    };
    Ok(Made { image, memory, allocation_size: requirements.size, handle })
}
