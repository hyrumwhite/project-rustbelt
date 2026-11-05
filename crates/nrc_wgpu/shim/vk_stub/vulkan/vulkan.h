/*
 * The few Vulkan declarations NrcVk.h needs, so the shim builds without the
 * Vulkan SDK. The handle typedefs match vulkan_core.h exactly (64-bit), which
 * matters: they are part of the mangled names NRC_Vulkan.lib exports, so any
 * difference shows up as an unresolved symbol at link time.
 */
#pragma once
#include <stdint.h>

#define VK_NULL_HANDLE nullptr
#define VK_DEFINE_HANDLE(object) typedef struct object##_T* object;
#define VK_DEFINE_NON_DISPATCHABLE_HANDLE(object) typedef struct object##_T* object;

VK_DEFINE_HANDLE(VkInstance)
VK_DEFINE_HANDLE(VkPhysicalDevice)
VK_DEFINE_HANDLE(VkDevice)
VK_DEFINE_HANDLE(VkQueue)
VK_DEFINE_HANDLE(VkCommandBuffer)
VK_DEFINE_NON_DISPATCHABLE_HANDLE(VkBuffer)
VK_DEFINE_NON_DISPATCHABLE_HANDLE(VkDeviceMemory)
VK_DEFINE_NON_DISPATCHABLE_HANDLE(VkImageView)

typedef uint32_t VkFlags;
typedef VkFlags VkBufferUsageFlags;
typedef uint64_t VkDeviceAddress;
