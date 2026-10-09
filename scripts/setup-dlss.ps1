# Sets up everything needed to build with `--features dlss`:
#   1. Verifies the Vulkan SDK and LLVM/clang are installed (needed by dlss_wgpu's build script)
#   2. Clones the NVIDIA DLSS SDK (v310.9.1, the version dlss_wgpu 6.0 expects) into vendor/DLSS
#   3. Copies the DLSS runtime DLLs next to the built executables
#
# By running this script you agree to the DLSS SDK license:
#   https://github.com/NVIDIA/DLSS/blob/v310.9.1/LICENSE.txt

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$sdk = Join-Path $root 'vendor/DLSS'
$dlssTag = 'v310.9.1'

# --- Prerequisites ---------------------------------------------------------
$missing = @()
if (-not $env:VULKAN_SDK -or -not (Test-Path $env:VULKAN_SDK)) {
    $missing += "Vulkan SDK  ->  winget install KhronosGroup.VulkanSDK   (sets VULKAN_SDK; restart your terminal)"
}
$libclang = @($env:LIBCLANG_PATH, 'C:\Program Files\LLVM\bin') |
    Where-Object { $_ -and (Test-Path (Join-Path $_ 'libclang.dll')) } | Select-Object -First 1
if (-not $libclang) {
    $missing += "LLVM/clang  ->  winget install LLVM.LLVM"
}
if ($missing) {
    Write-Warning "Missing build prerequisites for DLSS:"
    $missing | ForEach-Object { Write-Warning "  $_" }
}

# --- DLSS SDK --------------------------------------------------------------
if (-not (Test-Path (Join-Path $sdk 'include'))) {
    Write-Host "Cloning NVIDIA DLSS SDK $dlssTag into $sdk ..."
    git clone --depth 1 --branch $dlssTag https://github.com/NVIDIA/DLSS.git $sdk
} else {
    Write-Host "DLSS SDK already present at $sdk"
}

# --- Runtime DLLs ----------------------------------------------------------
$dllDir = Join-Path $sdk 'lib/Windows_x86_64/rel'
foreach ($profile in 'debug', 'release') {
    $out = Join-Path $root "target/$profile"
    New-Item -ItemType Directory -Force $out | Out-Null
    foreach ($dll in 'nvngx_dlss.dll', 'nvngx_dlssd.dll') {
        Copy-Item (Join-Path $dllDir $dll) $out -Force
    }
}
Write-Host "Copied nvngx_dlss.dll + nvngx_dlssd.dll into target/debug and target/release"

if (-not $missing) {
    Write-Host "`nReady. Run:  cargo run --release --features dlss"
}
