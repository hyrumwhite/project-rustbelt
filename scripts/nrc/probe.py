"""Loads NVIDIA's NRC_Vulkan.dll (no device needed) and prints what an
integration must know: the Vulkan extension and feature lists it asks for,
the training dimensions it recommends, its buffer sizes at 1280x720, and
whether Initialize accepts the installed driver.

Put NRC_Vulkan.dll and cudart64_13.dll (from https://github.com/NVIDIA-RTX/NRC,
Git LFS objects under Bin/x64) in a bin/ folder next to this file, then:

    python scripts/nrc/probe.py

Struct layouts follow NrcCommon.h v0.16 (x64 MSVC).
"""
import ctypes
import os
from ctypes import (POINTER, Structure, byref, c_bool, c_char_p, c_float,
                    c_int, c_size_t, c_uint32, c_void_p, c_wchar_p)

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.path.join(HERE, "bin")
os.add_dll_directory(BIN)
dll = ctypes.WinDLL(os.path.join(BIN, "NRC_Vulkan.dll"))

STATUS = ["OK", "SDKVersionMismatch", "AlreadyInitialized", "SDKNotInitialized", "InternalError",
          "MemoryNotProvided", "OutOfMemory", "AllocationFailed", "ErrorParsingJSON", "WrongParameter",
          "UnsupportedDriver", "UnsupportedHardware"]


def status(value):
    return STATUS[value] if 0 <= value < len(STATUS) else str(value)


def strings(export):
    f = getattr(dll, export)
    f.restype = c_uint32
    f.argtypes = [POINTER(POINTER(c_char_p))]
    arr = POINTER(c_char_p)()
    n = f(byref(arr))
    return [arr[i].decode() for i in range(n)]


print("instance extensions:", strings("?GetVulkanInstanceExtensions@vulkan@nrc@@YAIAEAPEBQEBD@Z"))
print("device extensions:  ", strings("?GetVulkanDeviceExtensions@vulkan@nrc@@YAIAEAPEBQEBD@Z"))
print("device features:    ", strings("?GetVulkanDeviceFeatures@vulkan@nrc@@YAIAEAPEBQEBD@Z"))


class U2(Structure):
    _fields_ = [("x", c_uint32), ("y", c_uint32)]


class F3(Structure):
    _fields_ = [("x", c_float), ("y", c_float), ("z", c_float)]


class ContextSettings(Structure):
    _fields_ = [("learnIrradiance", c_bool), ("includeDirectLighting", c_bool), ("requestReset", c_bool),
                ("sceneBoundsMin", F3), ("sceneBoundsMax", F3), ("smallestResolvableFeatureSize", c_float),
                ("frameDimensions", U2), ("trainingDimensions", U2), ("samplesPerPixel", c_uint32),
                ("maxPathVertices", c_uint32)]


class AllocationInfo(Structure):
    _fields_ = [("elementCount", c_size_t), ("elementSize", c_size_t), ("isOnSharedHeap", c_bool),
                ("allowUAV", c_bool), ("useReadbackHeap", c_bool), ("debugName", c_char_p)]


class BuffersAllocationInfo(Structure):
    _fields_ = [("info", AllocationInfo * 9)]


class GlobalSettings(Structure):
    _fields_ = [("majorVersion", c_int), ("minorVersion", c_int), ("loggerFn", c_void_p),
                ("memoryLoggerFn", c_void_p), ("allocatorFn", c_void_p), ("deallocatorFn", c_void_p),
                ("enableGPUMemoryAllocation", c_bool), ("enableDebugBuffers", c_bool),
                ("maxNumFramesInFlight", c_uint32), ("depsDirectoryPath", c_wchar_p)]


assert ctypes.sizeof(ContextSettings) == 56
assert ctypes.sizeof(AllocationInfo) == 32
assert ctypes.sizeof(GlobalSettings) == 56

dims = getattr(dll, "?ComputeIdealTrainingDimensions@nrc@@YA?AUnrc_uint2@@AEBU2@IM@Z")
dims.restype = U2
dims.argtypes = [POINTER(U2), c_uint32, c_float]
for w, h in [(1280, 720), (1920, 1080), (2560, 1440)]:
    for iterations in [0, 4, 2]:
        d = dims(byref(U2(w, h)), iterations, 0.0)
        print(f"training dims {w}x{h}, {iterations} iterations: {d.x}x{d.y} = {d.x * d.y} paths "
              f"({100.0 * d.x * d.y / (w * h):.2f}% of pixels)")

alloc = getattr(dll, "?GetBuffersAllocationInfo@Context@vulkan@nrc@@SA?AW4Status@3@AEBUContextSettings@3@AEAUBuffersAllocationInfo@3@@Z")
alloc.restype = c_int
alloc.argtypes = [POINTER(ContextSettings), POINTER(BuffersAllocationInfo)]
NAMES = ["Counter", "QueryPathInfo", "TrainingPathInfo", "TrainingPathVertices", "TrainingRadiance",
         "TrainingRadianceParams", "QueryRadiance", "QueryRadianceParams", "DebugTrainingPathInfo"]
for w, h, vertices in [(1280, 720, 8), (1280, 720, 4), (1920, 1080, 8)]:
    td = dims(byref(U2(w, h)), 0, 0.0)
    cs = ContextSettings(True, True, False, F3(-1000, -1000, -1000), F3(1000, 1000, 1000), 0.01,
                         U2(w, h), td, 1, vertices)
    out = BuffersAllocationInfo()
    st = alloc(byref(cs), byref(out))
    print(f"--- buffers at {w}x{h}, training {td.x}x{td.y}, {vertices} vertices: {status(st)}")
    total = 0
    for name, a in zip(NAMES, out.info):
        size = a.elementCount * a.elementSize
        total += size
        print(f"  {name:24s} {a.elementSize:4d} x {a.elementCount:9d} = {size / 1e6:8.2f} MB"
              f"  uav={a.allowUAV} readback={a.useReadbackHeap} shared={a.isOnSharedHeap}")
    print(f"  total {total / 1e6:.1f} MB")

LOGGER = ctypes.WINFUNCTYPE(None, c_char_p, c_int)


def log(message, level):
    print(f"  [nrc log {level}] {message.decode(errors='replace')}")


logger = LOGGER(log)
settings = GlobalSettings(0, 16, ctypes.cast(logger, c_void_p), None, None, None, True, False, 4, BIN)
init = getattr(dll, "?Initialize@vulkan@nrc@@YA?AW4Status@2@AEBUGlobalSettings@2@@Z")
init.restype = c_int
init.argtypes = [POINTER(GlobalSettings)]
print("Initialize on the installed driver:", status(init(byref(settings))))
shutdown = getattr(dll, "?Shutdown@vulkan@nrc@@YAXXZ")
shutdown.restype = None
shutdown()
