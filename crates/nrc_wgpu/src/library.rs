//! Loading NRC_Vulkan.dll, checking its signature, and the library-wide
//! `Initialize` / `Shutdown`.

use std::{
    ffi::{CStr, c_char},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{NrcError, Status, sys};

/// The library's file name; with `/DELAYLOAD` the loader resolves it by this
/// name to the copy [`load_library`] already loaded.
pub const NRC_DLL: &str = "NRC_Vulkan.dll";

/// The DLLs the library needs at run time, all in one directory.
pub const RUNTIME_DLLS: [&str; 4] = [
    "NRC_Vulkan.dll",
    "cudart64_13.dll",
    "nvrtc64_130_0.dll",
    "nvrtc-builtins64_134.dll",
];

/// `<NRC_SDK>/Bin/x64` as found at build time: where the four runtime DLLs
/// are on the machine that built this crate.
pub fn sdk_bin_dir() -> &'static Path {
    Path::new(env!("NRC_SDK_BIN_DIR"))
}

/// Where the DLL was loaded from, once loaded.
static LOADED_FROM: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Loads `NRC_Vulkan.dll` from `directory`, which must also hold the CUDA
/// DLLs ([`RUNTIME_DLLS`]). Call this before anything else in the crate;
/// later calls return `Ok` (the first directory wins).
///
/// With `verify_signature`, each of the four DLLs present must carry a valid
/// Authenticode signature (`WinVerifyTrust`, as `NrcSecurity.h` does) or
/// nothing is loaded.
///
/// The crate links `NRC_Vulkan.lib` with `/DELAYLOAD`, so the binary starts
/// without the DLL; the first NRC call resolves to the module loaded here
/// (`LOAD_WITH_ALTERED_SEARCH_PATH` lets it find `cudart64_13.dll` beside it).
pub fn load_library(directory: &Path, verify_signature: bool) -> Result<(), NrcError> {
    let mut loaded = LOADED_FROM.lock().unwrap();
    if loaded.is_some() {
        return Ok(());
    }
    let dll = directory.join(NRC_DLL);
    if !dll.is_file() {
        return Err(NrcError::LibraryNotFound(dll));
    }
    if verify_signature {
        for name in RUNTIME_DLLS {
            let path = directory.join(name);
            if path.is_file() && !verify_signature_of(&path) {
                return Err(NrcError::BadSignature(path));
            }
        }
    }
    let wide = wide_nul(&dll);
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    // Loading runs the DLL's initializers, which is what the caller asked for.
    let module = unsafe {
        windows_sys::Win32::System::LibraryLoader::LoadLibraryExW(
            wide.as_ptr(),
            ptr::null_mut(),
            windows_sys::Win32::System::LibraryLoader::LOAD_WITH_ALTERED_SEARCH_PATH,
        )
    };
    if module.is_null() {
        return Err(NrcError::LoadFailed(dll, std::io::Error::last_os_error()));
    }
    // (the module is never freed: the delay-load thunks point into it)
    log::info!(target: "nrc", "loaded {}", dll.display());
    *loaded = Some(directory.to_path_buf());
    Ok(())
}

/// Checks the DLL is in the process before a delay-loaded call: without it
/// the delay-load helper raises an exception instead of returning an error.
pub(crate) fn ensure_loaded() -> Result<(), NrcError> {
    if LOADED_FROM.lock().unwrap().is_some() {
        return Ok(());
    }
    // (a program may also have loaded it by its own means, e.g. next to the exe)
    let name = wide_nul(Path::new(NRC_DLL));
    // SAFETY: a NUL-terminated module name; GetModuleHandleW only looks it up.
    let module = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(name.as_ptr()) };
    if module.is_null() { Err(NrcError::NotLoaded) } else { Ok(()) }
}

/// Whether `path` has a valid Authenticode signature chaining to a trusted
/// root, the check in the SDK's `NrcSecurity.h` (no revocation check, no UI,
/// the OS's current strong-signature policy).
pub fn verify_signature_of(path: &Path) -> bool {
    use windows_sys::Win32::Security::{Cryptography::*, WinTrust::*};

    let wide = wide_nul(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide.as_ptr(),
        hFile: ptr::null_mut(),
        pgKnownSubject: ptr::null_mut(),
    };
    // (szOID_CERT_STRONG_SIGN_OS_CURRENT, as the narrow string the field takes)
    let mut policy = CERT_STRONG_SIGN_PARA {
        cbSize: size_of::<CERT_STRONG_SIGN_PARA>() as u32,
        dwInfoChoice: CERT_STRONG_SIGN_OID_INFO_CHOICE,
        Anonymous: CERT_STRONG_SIGN_PARA_0 { pszOID: c"1.3.6.1.4.1.311.72.1.1".as_ptr() as *mut u8 },
    };
    let mut signature_settings = WINTRUST_SIGNATURE_SETTINGS {
        cbStruct: size_of::<WINTRUST_SIGNATURE_SETTINGS>() as u32,
        dwIndex: 0,
        dwFlags: WSS_GET_SECONDARY_SIG_COUNT | WSS_VERIFY_SPECIFIC,
        cSecondarySigs: 0,
        dwVerifiedSigIndex: 0,
        pCryptoPolicy: &mut policy,
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        pSignatureSettings: &mut signature_settings,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: every pointer in `data` points at a local that outlives both
    // calls; the second call releases the state the first one allocated.
    unsafe {
        let status = WinVerifyTrust(ptr::null_mut(), &mut action, (&raw mut data).cast());
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(ptr::null_mut(), &mut action, (&raw mut data).cast());
        status == 0
    }
}

fn wide_nul(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Library-wide settings for [`Nrc::initialize`].
#[derive(Clone, Debug)]
pub struct NrcInitSettings {
    /// Where `cudart64_13.dll` and the NVRTC DLLs are (`depsDirectoryPath`);
    /// normally the directory given to [`load_library`].
    pub deps_directory: PathBuf,
    /// Allocate the debug buffers the debug resolve modes need.
    pub enable_debug_buffers: bool,
    /// The most frames the renderer has in flight.
    pub max_frames_in_flight: u32,
}

impl NrcInitSettings {
    /// The SDK's defaults with the DLLs in `deps_directory`.
    pub fn new(deps_directory: impl Into<PathBuf>) -> Self {
        Self { deps_directory: deps_directory.into(), enable_debug_buffers: false, max_frames_in_flight: 4 }
    }
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// The initialized library (`nrc::vulkan::Initialize`). One per process at
/// a time; contexts hold an `Arc` of it, and `Shutdown` runs when the last
/// one goes.
///
/// Memory is always app-managed (`enableGPUMemoryAllocation = false`): each
/// [`crate::NrcContext`] allocates its buffers and exposes them as wgpu buffers.
#[derive(Debug)]
pub struct Nrc {
    // (the path the library was given; kept alive in case it keeps the pointer)
    _deps_directory: Vec<u16>,
}

impl Nrc {
    /// Initializes the library. [`load_library`] must have succeeded.
    /// Messages go to the `log` crate under the target `nrc`.
    pub fn initialize(settings: &NrcInitSettings) -> Result<Arc<Nrc>, NrcError> {
        ensure_loaded()?;
        if INITIALIZED.swap(true, Ordering::AcqRel) {
            return Err(NrcError::Status(Status::AlreadyInitialized));
        }
        let deps = wide_nul(&settings.deps_directory);
        let global = sys::GlobalSettings {
            major_version: sys::NRC_VERSION_MAJOR,
            minor_version: sys::NRC_VERSION_MINOR,
            logger_fn: Some(log_message),
            memory_logger_fn: ptr::null(),
            allocator_fn: ptr::null(),
            deallocator_fn: ptr::null(),
            enable_gpu_memory_allocation: false,
            enable_debug_buffers: settings.enable_debug_buffers,
            max_num_frames_in_flight: settings.max_frames_in_flight,
            deps_directory_path: deps.as_ptr(),
        };
        // SAFETY: the DLL is loaded (ensure_loaded); `global` matches
        // nrc::GlobalSettings (layout test) and its path outlives the library
        // (kept in the returned Nrc).
        let status = Status::from_raw(unsafe { sys::nrc_shim_initialize(&global) });
        if status != Status::Ok {
            INITIALIZED.store(false, Ordering::Release);
            return Err(NrcError::Status(status));
        }
        Ok(Arc::new(Nrc { _deps_directory: deps }))
    }
}

impl Drop for Nrc {
    fn drop(&mut self) {
        // SAFETY: Initialize succeeded; contexts hold an Arc<Nrc>, so all are destroyed by now.
        unsafe { sys::nrc_shim_shutdown() };
        INITIALIZED.store(false, Ordering::Release);
    }
}

/// `GlobalSettings::loggerFn`: the library's messages into `log`.
unsafe extern "C" fn log_message(message: *const c_char, level: i32) {
    if message.is_null() {
        return;
    }
    // SAFETY: the library passes a NUL-terminated string valid for the call.
    let text = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    let text = text.trim_end();
    match level {
        1 => log::debug!(target: "nrc", "{text}"),
        2 => log::info!(target: "nrc", "{text}"),
        3 => log::warn!(target: "nrc", "{text}"),
        _ => log::error!(target: "nrc", "{text}"),
    }
}

/// Copies a list of static strings the library returns.
pub(crate) fn static_strings(count: u32, names: *const *const c_char) -> Vec<&'static CStr> {
    if names.is_null() {
        return Vec::new();
    }
    (0..count as usize)
        // SAFETY: the library returns `count` pointers to NUL-terminated
        // strings in its static data; the DLL is never unloaded, so 'static holds.
        .map(|i| unsafe { CStr::from_ptr(*names.add(i)) })
        .collect()
}
