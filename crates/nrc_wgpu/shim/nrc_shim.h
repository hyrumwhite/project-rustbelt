/*
 * A C interface over NVIDIA's NRC Vulkan C++ API (NrcVk.h), for Rust.
 *
 * Every function returns the nrc::Status value as an int (0 = OK) unless
 * stated otherwise. Struct pointers point at the real nrc:: structs; the Rust
 * side mirrors them with #[repr(C)] types, and nrc_shim_layout() reports the
 * C++ sizes and offsets so the two can be compared in a test. Vulkan handles
 * travel as uint64_t (ash Handle::as_raw).
 */
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int nrc_shim_initialize(const void* global_settings);
void nrc_shim_shutdown(void);

uint32_t nrc_shim_get_instance_extensions(const char* const** out_names);
uint32_t nrc_shim_get_device_extensions(const char* const** out_names);
uint32_t nrc_shim_get_device_features(const char* const** out_names);

/* Writes the ideal training size for a frame size into out_dimensions[0..2]. */
void nrc_shim_compute_ideal_training_dimensions(const uint32_t frame_dimensions[2], uint32_t num_training_iterations,
                                                float avg_training_vertices_per_path, uint32_t out_dimensions[2]);

int nrc_shim_context_create(uint64_t device, uint64_t physical_device, uint64_t instance, void** out_context);
int nrc_shim_context_destroy(void* context);
int nrc_shim_get_buffers_allocation_info(const void* context_settings, void* out_allocation_info);
int nrc_shim_context_configure(void* context, const void* context_settings, const void* buffers,
                               const char* network_config_filename);
int nrc_shim_context_populate_shader_constants(const void* context, void* out_constants);
int nrc_shim_context_begin_frame(void* context, uint64_t command_buffer, const void* frame_settings);
int nrc_shim_context_query_and_train(void* context, uint64_t command_buffer, float* training_loss);
int nrc_shim_context_resolve(void* context, uint64_t command_buffer, uint64_t output_image_view);
int nrc_shim_context_train(void* context, uint64_t command_buffer, float* training_loss);
int nrc_shim_context_end_frame(void* context, uint64_t queue);
/* The library's view of the buffers (nrc::vulkan::Buffers), or NULL. */
const void* nrc_shim_context_get_buffers(const void* context);
/* VkBufferUsageFlags the library wants for a buffer. */
uint32_t nrc_shim_context_get_buffer_usage_flags(const void* context, const void* allocation_info);

/* Size or offset number `index` of the C++ structs (see the table in the .cpp); ~0 past the end. */
uint64_t nrc_shim_layout(uint32_t index);

#ifdef __cplusplus
}
#endif
