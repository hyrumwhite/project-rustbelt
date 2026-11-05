//! Instance and device creation with what NRC needs, through wgpu-hal's
//! creation callbacks (the same shape as `dlss_wgpu`).
//!
//! The lists come from the library at run time (`GetVulkanInstanceExtensions`,
//! `GetVulkanDeviceExtensions`, `GetVulkanDeviceFeatures`), so
//! [`crate::load_library`] must have run first.

use std::ffi::CStr;

use ash::vk;
use wgpu::{
    Adapter, Device, DeviceDescriptor, Instance, InstanceDescriptor, Limits, Queue,
    hal::{
        api::Vulkan,
        vulkan::{CreateDeviceCallbackArgs, CreateInstanceCallbackArgs},
    },
};

use crate::{NrcError, library, sys};

/// The Vulkan instance extensions NRC asks for.
pub fn required_instance_extensions() -> Result<Vec<&'static CStr>, NrcError> {
    library::ensure_loaded()?;
    let mut names = std::ptr::null();
    // SAFETY: the DLL is loaded; the call writes a pointer to a static array.
    let count = unsafe { sys::nrc_shim_get_instance_extensions(&mut names) };
    Ok(library::static_strings(count, names))
}

/// The Vulkan device extensions NRC asks for.
pub fn required_device_extensions() -> Result<Vec<&'static CStr>, NrcError> {
    library::ensure_loaded()?;
    let mut names = std::ptr::null();
    // SAFETY: as above.
    let count = unsafe { sys::nrc_shim_get_device_extensions(&mut names) };
    Ok(library::static_strings(count, names))
}

/// The Vulkan device features NRC asks for, by the library's names: the
/// feature structs' sType names, such as
/// `VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES`.
pub fn required_device_features() -> Result<Vec<&'static CStr>, NrcError> {
    library::ensure_loaded()?;
    let mut names = std::ptr::null();
    // SAFETY: as above.
    let count = unsafe { sys::nrc_shim_get_device_features(&mut names) };
    Ok(library::static_strings(count, names))
}

/// Creates a Vulkan wgpu [`Instance`] with NRC's instance extensions.
pub fn create_instance(instance_descriptor: &InstanceDescriptor) -> Result<Instance, NrcError> {
    let mut result = Ok(());
    // SAFETY: the callback only adds extensions the loader reports as
    // supported (register_instance_extensions checks).
    let raw_instance = unsafe {
        wgpu::hal::vulkan::Instance::init_with_callback(
            &wgpu::hal::InstanceDescriptor {
                name: "wgpu",
                flags: instance_descriptor.flags,
                memory_budget_thresholds: instance_descriptor.memory_budget_thresholds,
                backend_options: instance_descriptor.backend_options.clone(),
                telemetry: None,
                display: None,
            },
            Some(Box::new(|mut args| {
                result = register_instance_extensions(&mut args);
            })),
        )
    }
    .map_err(|e| NrcError::Instance(e.to_string()))?;
    result?;
    // SAFETY: a valid hal instance, handed over.
    Ok(unsafe { Instance::from_hal::<Vulkan>(raw_instance) })
}

/// Adds NRC's instance extensions; call it from inside
/// [`wgpu::hal::vulkan::Instance::init_with_callback`] (or Bevy's
/// `RawVulkanInitSettings::add_create_instance_callback`).
pub fn register_instance_extensions(args: &mut CreateInstanceCallbackArgs) -> Result<(), NrcError> {
    let required = required_instance_extensions()?;
    // SAFETY: a plain query through the loader wgpu created.
    let supported = unsafe { args.entry.enumerate_instance_extension_properties(None) }
        .map_err(NrcError::Vulkan)?;
    for name in required {
        if !supported.iter().any(|p| p.extension_name_as_c_str() == Ok(name)) {
            return Err(NrcError::MissingInstanceExtension(name.to_string_lossy().into_owned()));
        }
        if !args.extensions.contains(&name) {
            args.extensions.push(name);
        }
    }
    Ok(())
}

/// Creates a wgpu [`Device`] and [`Queue`] on a Vulkan `adapter` with NRC's
/// device extensions and features enabled (buffer device address included).
pub fn request_device(
    adapter: &Adapter,
    device_descriptor: &DeviceDescriptor,
    limits: Option<Limits>,
) -> Result<(Device, Queue), NrcError> {
    // SAFETY: the hal adapter is only used within this function, while `adapter` lives.
    let raw_adapter = unsafe { adapter.as_hal::<Vulkan>() }.ok_or(NrcError::NotVulkan)?;
    let mut result = Ok(());
    // SAFETY: the callback adds only supported extensions and features
    // (register_device_extensions checks) and turns nothing off.
    let open_device = unsafe {
        raw_adapter.open_with_callback(
            device_descriptor.required_features,
            &limits.unwrap_or(adapter.limits()),
            &device_descriptor.memory_hints,
            Some(Box::new(|mut args| {
                result = register_device_extensions(&mut args, &raw_adapter);
            })),
        )
    }
    .map_err(|e| NrcError::Device(e.to_string()))?;
    result?;
    // SAFETY: the device was opened from this adapter with the descriptor's features.
    unsafe { adapter.create_device_from_hal::<Vulkan>(open_device, device_descriptor) }
        .map_err(|e| NrcError::Device(e.to_string()))
}

/// Adds NRC's device extensions and features; call it from inside
/// [`wgpu::hal::vulkan::Adapter::open_with_callback`] (or Bevy's
/// `RawVulkanInitSettings::add_create_device_callback`).
///
/// Features go into the `create_info` pNext chain: a feature struct already
/// in the chain is switched on in place, otherwise a new one is added. The
/// added structs must outlive device creation, so they are leaked (a few
/// dozen bytes per device created).
pub fn register_device_extensions(
    args: &mut CreateDeviceCallbackArgs,
    raw_adapter: &wgpu::hal::vulkan::Adapter,
) -> Result<(), NrcError> {
    let capabilities = raw_adapter.physical_device_capabilities();
    let bda_enabled_by_wgpu = args.extensions.contains(&ash::khr::buffer_device_address::NAME);
    for name in required_device_extensions()? {
        if !capabilities.supports_extension(name) {
            return Err(NrcError::MissingDeviceExtension(name.to_string_lossy().into_owned()));
        }
        if !args.extensions.contains(&name) {
            args.extensions.push(name);
        }
    }

    // What the physical device supports, for the features NRC can ask for.
    let instance = raw_adapter.shared_instance().raw_instance();
    let mut scalar = vk::PhysicalDeviceScalarBlockLayoutFeatures::default();
    let mut standard = vk::PhysicalDeviceUniformBufferStandardLayoutFeatures::default();
    let mut bda = vk::PhysicalDeviceBufferDeviceAddressFeatures::default();
    let mut features2 = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut scalar)
        .push_next(&mut standard)
        .push_next(&mut bda);
    // SAFETY: a query on the adapter's physical device with a valid chain.
    unsafe { instance.get_physical_device_features2(raw_adapter.raw_physical_device(), &mut features2) };
    let (scalar, standard, bda) = (scalar.scalar_block_layout, standard.uniform_buffer_standard_layout, bda.buffer_device_address);

    let mut wanted = Vec::new();
    for feature in required_device_features()? {
        let name = feature.to_string_lossy();
        let lower = name.to_ascii_lowercase().replace('_', "");
        let (s_type, supported) = if lower.contains("scalarblocklayout") {
            (vk::StructureType::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES, scalar)
        } else if lower.contains("uniformbufferstandardlayout") {
            (vk::StructureType::PHYSICAL_DEVICE_UNIFORM_BUFFER_STANDARD_LAYOUT_FEATURES, standard)
        } else if lower.contains("bufferdeviceaddress") {
            (vk::StructureType::PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES, bda)
        } else {
            return Err(NrcError::UnknownDeviceFeature(name.into_owned()));
        };
        if supported == vk::FALSE {
            return Err(NrcError::MissingDeviceFeature(name.into_owned()));
        }
        wanted.push(s_type);
    }
    // NRC's buffers need device addresses (VkBufferDeviceAddressInfo in
    // NrcContext). wgpu chains its own struct only when it enabled the
    // extension itself (ray queries); otherwise this adds one.
    // (wgpu adds its struct after this callback returns, so it is not in the
    // chain yet; a second one would be invalid)
    let bda_type = vk::StructureType::PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES;
    if bda_enabled_by_wgpu {
        wanted.retain(|&s_type| s_type != bda_type);
    } else {
        if bda == vk::FALSE {
            return Err(NrcError::MissingDeviceFeature("bufferDeviceAddress".into()));
        }
        wanted.push(bda_type);
    }
    wanted.sort_by_key(|s| s.as_raw());
    wanted.dedup();

    for s_type in wanted {
        // SAFETY: walking the pNext chain the callback was given; every
        // element starts with sType/pNext (VkBaseOutStructure).
        if unsafe { enable_in_chain(args.create_info.p_next as *mut vk::BaseOutStructure, s_type) } {
            continue;
        }
        let create_info = &mut *args.create_info;
        *create_info = match s_type {
            vk::StructureType::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES => create_info.push_next(Box::leak(Box::new(
                vk::PhysicalDeviceScalarBlockLayoutFeatures::default().scalar_block_layout(true),
            ))),
            vk::StructureType::PHYSICAL_DEVICE_UNIFORM_BUFFER_STANDARD_LAYOUT_FEATURES => create_info.push_next(Box::leak(Box::new(
                vk::PhysicalDeviceUniformBufferStandardLayoutFeatures::default().uniform_buffer_standard_layout(true),
            ))),
            _ => create_info.push_next(Box::leak(Box::new(
                vk::PhysicalDeviceBufferDeviceAddressFeatures::default().buffer_device_address(true),
            ))),
        };
    }

    // NRC's own pipeline barriers name VK_PIPELINE_STAGE_RAY_TRACING_SHADER_BIT_KHR,
    // which is only valid with the rayTracingPipeline feature (validation
    // reports VUID-vkCmdPipelineBarrier-srcStageMask-07949 otherwise). The
    // library does not list it, so it is enabled here when the GPU has it;
    // wgpu chains its own structs for the extensions it enabled itself.
    let rt_pipeline = ash::khr::ray_tracing_pipeline::NAME;
    if capabilities.supports_extension(rt_pipeline) && !args.extensions.contains(&rt_pipeline) {
        let create_info = &mut *args.create_info;
        if !args.extensions.contains(&ash::khr::acceleration_structure::NAME) {
            for name in [ash::khr::deferred_host_operations::NAME, ash::khr::acceleration_structure::NAME] {
                if !args.extensions.contains(&name) {
                    args.extensions.push(name);
                }
            }
            *create_info = create_info.push_next(Box::leak(Box::new(
                vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default().acceleration_structure(true),
            )));
        }
        args.extensions.push(rt_pipeline);
        *create_info = create_info.push_next(Box::leak(Box::new(
            vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default().ray_tracing_pipeline(true),
        )));
    }
    Ok(())
}

/// Switches on the feature in a struct of type `s_type` already in the chain;
/// false if there is none.
///
/// # Safety
/// `node` must be null or a valid pNext chain of Vulkan structs.
unsafe fn enable_in_chain(mut node: *mut vk::BaseOutStructure, s_type: vk::StructureType) -> bool {
    while !node.is_null() {
        // SAFETY: the caller guarantees a valid chain; each struct is cast to
        // its real type by its sType before a field is written.
        unsafe {
            if (*node).s_type == s_type {
                match s_type {
                    vk::StructureType::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES => {
                        (*(node as *mut vk::PhysicalDeviceScalarBlockLayoutFeatures)).scalar_block_layout = vk::TRUE
                    }
                    vk::StructureType::PHYSICAL_DEVICE_UNIFORM_BUFFER_STANDARD_LAYOUT_FEATURES => {
                        (*(node as *mut vk::PhysicalDeviceUniformBufferStandardLayoutFeatures))
                            .uniform_buffer_standard_layout = vk::TRUE
                    }
                    _ => (*(node as *mut vk::PhysicalDeviceBufferDeviceAddressFeatures)).buffer_device_address = vk::TRUE,
                }
                return true;
            }
            // (a Vulkan12Features struct would also carry these; enable there too)
            if (*node).s_type == vk::StructureType::PHYSICAL_DEVICE_VULKAN_1_2_FEATURES {
                let v12 = &mut *(node as *mut vk::PhysicalDeviceVulkan12Features);
                match s_type {
                    vk::StructureType::PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES => v12.scalar_block_layout = vk::TRUE,
                    vk::StructureType::PHYSICAL_DEVICE_UNIFORM_BUFFER_STANDARD_LAYOUT_FEATURES => {
                        v12.uniform_buffer_standard_layout = vk::TRUE
                    }
                    _ => v12.buffer_device_address = vk::TRUE,
                }
                return true;
            }
            node = (*node).p_next;
        }
    }
    false
}
