// C shim over nrc::vulkan (NrcVk.h). See nrc_shim.h.
//
// The calls are wrapped in try/catch so a C++ exception can never unwind into
// Rust; one becomes nrc::Status::InternalError.

#include <stddef.h>
#include <stdint.h>

#include "NrcVk.h"
#include "NrcStructures.h"
#include "nrc_shim.h"

using namespace nrc;

// The layouts the Rust mirrors assume (x64 MSVC). A change in the SDK headers
// fails here first.
static_assert(sizeof(GlobalSettings) == 56, "GlobalSettings layout changed");
static_assert(sizeof(ContextSettings) == 56, "ContextSettings layout changed");
static_assert(sizeof(FrameSettings) == 60, "FrameSettings layout changed");
static_assert(sizeof(AllocationInfo) == 32, "AllocationInfo layout changed");
static_assert(sizeof(BuffersAllocationInfo) == 9 * 32, "BuffersAllocationInfo layout changed");
static_assert(sizeof(vulkan::BufferInfo) == 40, "BufferInfo layout changed");
static_assert(sizeof(vulkan::Buffers) == 9 * 40, "Buffers layout changed");
static_assert(sizeof(NrcConstants) == 96, "NrcConstants layout changed");
static_assert((int)BufferIdx::Count == 9, "BufferIdx changed");

#define NRC_SHIM_TRY(expr)                                                                                             \
    try                                                                                                                \
    {                                                                                                                  \
        return (int)(expr);                                                                                            \
    }                                                                                                                  \
    catch (...)                                                                                                        \
    {                                                                                                                  \
        return (int)Status::InternalError;                                                                             \
    }

template <typename T> static T handle(uint64_t raw)
{
    return reinterpret_cast<T>(static_cast<uintptr_t>(raw));
}

static vulkan::Context* ctx(void* context)
{
    return static_cast<vulkan::Context*>(context);
}

static const vulkan::Context* cctx(const void* context)
{
    return static_cast<const vulkan::Context*>(context);
}

extern "C" {

int nrc_shim_initialize(const void* global_settings)
{
    NRC_SHIM_TRY(vulkan::Initialize(*static_cast<const GlobalSettings*>(global_settings)));
}

void nrc_shim_shutdown(void)
{
    try
    {
        vulkan::Shutdown();
    }
    catch (...)
    {
    }
}

uint32_t nrc_shim_get_instance_extensions(const char* const** out_names)
{
    *out_names = nullptr;
    try
    {
        return vulkan::GetVulkanInstanceExtensions(*out_names);
    }
    catch (...)
    {
        return 0;
    }
}

uint32_t nrc_shim_get_device_extensions(const char* const** out_names)
{
    *out_names = nullptr;
    try
    {
        return vulkan::GetVulkanDeviceExtensions(*out_names);
    }
    catch (...)
    {
        return 0;
    }
}

uint32_t nrc_shim_get_device_features(const char* const** out_names)
{
    *out_names = nullptr;
    try
    {
        return vulkan::GetVulkanDeviceFeatures(*out_names);
    }
    catch (...)
    {
        return 0;
    }
}

void nrc_shim_compute_ideal_training_dimensions(const uint32_t frame_dimensions[2], uint32_t num_training_iterations,
                                                float avg_training_vertices_per_path, uint32_t out_dimensions[2])
{
    nrc_uint2 frame = {frame_dimensions[0], frame_dimensions[1]};
    nrc_uint2 result = {0, 0};
    try
    {
        result = ComputeIdealTrainingDimensions(frame, num_training_iterations, avg_training_vertices_per_path);
    }
    catch (...)
    {
    }
    out_dimensions[0] = result.x;
    out_dimensions[1] = result.y;
}

int nrc_shim_context_create(uint64_t device, uint64_t physical_device, uint64_t instance, void** out_context)
{
    *out_context = nullptr;
    try
    {
        vulkan::Context* created = nullptr;
        Status status = vulkan::Context::Create(handle<VkDevice>(device), handle<VkPhysicalDevice>(physical_device),
                                                handle<VkInstance>(instance), created);
        *out_context = created;
        return (int)status;
    }
    catch (...)
    {
        return (int)Status::InternalError;
    }
}

int nrc_shim_context_destroy(void* context)
{
    NRC_SHIM_TRY(vulkan::Context::Destroy(*ctx(context)));
}

int nrc_shim_get_buffers_allocation_info(const void* context_settings, void* out_allocation_info)
{
    NRC_SHIM_TRY(vulkan::Context::GetBuffersAllocationInfo(*static_cast<const ContextSettings*>(context_settings),
                                                           *static_cast<BuffersAllocationInfo*>(out_allocation_info)));
}

int nrc_shim_context_configure(void* context, const void* context_settings, const void* buffers,
                               const char* network_config_filename)
{
    NRC_SHIM_TRY(ctx(context)->Configure(*static_cast<const ContextSettings*>(context_settings),
                                         static_cast<const vulkan::Buffers*>(buffers), network_config_filename));
}

int nrc_shim_context_populate_shader_constants(const void* context, void* out_constants)
{
    NRC_SHIM_TRY(cctx(context)->PopulateShaderConstants(*static_cast<NrcConstants*>(out_constants)));
}

int nrc_shim_context_begin_frame(void* context, uint64_t command_buffer, const void* frame_settings)
{
    NRC_SHIM_TRY(ctx(context)->BeginFrame(handle<VkCommandBuffer>(command_buffer),
                                          *static_cast<const FrameSettings*>(frame_settings)));
}

int nrc_shim_context_query_and_train(void* context, uint64_t command_buffer, float* training_loss)
{
    NRC_SHIM_TRY(ctx(context)->QueryAndTrain(handle<VkCommandBuffer>(command_buffer), training_loss));
}

int nrc_shim_context_resolve(void* context, uint64_t command_buffer, uint64_t output_image_view)
{
    NRC_SHIM_TRY(ctx(context)->Resolve(handle<VkCommandBuffer>(command_buffer), handle<VkImageView>(output_image_view)));
}

int nrc_shim_context_train(void* context, uint64_t command_buffer, float* training_loss)
{
    NRC_SHIM_TRY(ctx(context)->Train(handle<VkCommandBuffer>(command_buffer), training_loss));
}

int nrc_shim_context_end_frame(void* context, uint64_t queue)
{
    NRC_SHIM_TRY(ctx(context)->EndFrame(handle<VkQueue>(queue)));
}

const void* nrc_shim_context_get_buffers(const void* context)
{
    try
    {
        return cctx(context)->GetBuffers();
    }
    catch (...)
    {
        return nullptr;
    }
}

uint32_t nrc_shim_context_get_buffer_usage_flags(const void* context, const void* allocation_info)
{
    try
    {
        return cctx(context)->GetBufferUsageFlags(*static_cast<const AllocationInfo*>(allocation_info));
    }
    catch (...)
    {
        return 0;
    }
}

uint64_t nrc_shim_layout(uint32_t index)
{
    // Keep in the same order as LAYOUT in src/sys.rs.
    static const uint64_t table[] = {
        sizeof(GlobalSettings),
        offsetof(GlobalSettings, loggerFn),
        offsetof(GlobalSettings, enableGPUMemoryAllocation),
        offsetof(GlobalSettings, enableDebugBuffers),
        offsetof(GlobalSettings, maxNumFramesInFlight),
        offsetof(GlobalSettings, depsDirectoryPath),
        sizeof(ContextSettings),
        offsetof(ContextSettings, requestReset),
        offsetof(ContextSettings, sceneBoundsMin),
        offsetof(ContextSettings, sceneBoundsMax),
        offsetof(ContextSettings, smallestResolvableFeatureSize),
        offsetof(ContextSettings, frameDimensions),
        offsetof(ContextSettings, trainingDimensions),
        offsetof(ContextSettings, samplesPerPixel),
        offsetof(ContextSettings, maxPathVertices),
        sizeof(FrameSettings),
        offsetof(FrameSettings, skipDeltaVertices),
        offsetof(FrameSettings, terminationHeuristicThreshold),
        offsetof(FrameSettings, resolveMode),
        offsetof(FrameSettings, trainTheCache),
        offsetof(FrameSettings, usedTrainingDimensions),
        offsetof(FrameSettings, proportionPrimarySegmentsToTrainOn),
        offsetof(FrameSettings, proportionUnbiased),
        offsetof(FrameSettings, selfTrainingAttenuation),
        offsetof(FrameSettings, numTrainingIterations),
        offsetof(FrameSettings, learningRate),
        sizeof(AllocationInfo),
        offsetof(AllocationInfo, isOnSharedHeap),
        offsetof(AllocationInfo, useReadbackHeap),
        offsetof(AllocationInfo, debugName),
        sizeof(vulkan::BufferInfo),
        offsetof(vulkan::BufferInfo, allocatedOffset),
        offsetof(vulkan::BufferInfo, memory),
        offsetof(vulkan::BufferInfo, deviceAddress),
        sizeof(NrcConstants),
        offsetof(NrcConstants, scenePosScale),
        offsetof(NrcConstants, maxPathVertices),
        offsetof(NrcConstants, resolveMode),
        offsetof(NrcConstants, proportionUnbiased),
    };
    if (index >= sizeof(table) / sizeof(table[0]))
        return ~0ull;
    return table[index];
}

} // extern "C"
