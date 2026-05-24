//! Glass Helper Process
//!
//! This is the helper executable for CEF subprocesses (GPU, Renderer, Plugin, etc.)
//! It must be bundled as separate .app bundles in Contents/Frameworks/

#[cfg(target_os = "windows")]
fn resolve_windows_cef_dir() -> std::path::PathBuf {
    let cef_dir = match std::env::var("CEF_PATH") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("cef_runtime")))
            .unwrap_or_else(|| std::path::PathBuf::from("cef_runtime")),
    };

    if cef_dir.join("libcef.dll").exists() {
        cef_dir
    } else {
        eprintln!("libcef.dll not found at {}", cef_dir.join("libcef.dll").display());
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
fn configure_windows_cef_dll_directory(cef_dir: &std::path::Path) {
    use windows::Win32::System::LibraryLoader::SetDllDirectoryW;
    use windows::core::HSTRING;

    let Some(cef_dir) = cef_dir.to_str() else {
        eprintln!("CEF runtime path is not valid UTF-8: {}", cef_dir.display());
        std::process::exit(1);
    };

    if let Err(error) = unsafe { SetDllDirectoryW(&HSTRING::from(cef_dir)) } {
        eprintln!("Failed to set CEF DLL directory: {error}");
        std::process::exit(1);
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    {
        use cef::library_loader::LibraryLoader;

        let exe_path = std::env::current_exe().expect("failed to get current exe path");

        // Try bundle-relative path first (helper is at
        // Glass Helper.app/Contents/MacOS/Glass Helper, framework is 3 levels up)
        let bundle_framework = exe_path.parent().map(|p| {
            p.join("../../../Chromium Embedded Framework.framework/Chromium Embedded Framework")
        });

        let loaded = match bundle_framework {
            Some(ref path) if path.exists() => {
                let loader = LibraryLoader::new(&exe_path, true);
                if loader.load() {
                    std::mem::forget(loader);
                    true
                } else {
                    false
                }
            }
            _ => false,
        };

        // Fall back to CEF_PATH env var or ~/.local/share/cef
        if !loaded {
            let cef_dir = match std::env::var("CEF_PATH") {
                Ok(path) => std::path::PathBuf::from(path),
                Err(_) => {
                    let home = std::env::var("HOME").expect("HOME not set");
                    std::path::PathBuf::from(home).join(".local/share/cef")
                }
            };

            let framework_path =
                cef_dir.join("Chromium Embedded Framework.framework/Chromium Embedded Framework");

            if !framework_path.exists() {
                eprintln!(
                    "CEF framework not found at bundle path or {}",
                    framework_path.display()
                );
                std::process::exit(1);
            }

            use std::os::unix::ffi::OsStrExt;
            let path_cstr = std::ffi::CString::new(framework_path.as_os_str().as_bytes())
                .expect("invalid CEF path");
            let result = unsafe { cef::load_library(Some(&*path_cstr.as_ptr().cast())) };
            if result != 1 {
                eprintln!(
                    "Failed to load CEF library from {}",
                    framework_path.display()
                );
                std::process::exit(1);
            }
        }

        // Initialize CEF API
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

        let args = cef::args::Args::new();
        let mut app = browser::build_cef_app();

        // Execute the subprocess - this handles GPU, Renderer, etc.
        let exit_code = cef::execute_process(
            Some(args.as_main_args()),
            Some(&mut app),
            std::ptr::null_mut(),
        );

        // exit_code >= 0 means this was a subprocess, exit with that code
        if exit_code >= 0 {
            std::process::exit(exit_code);
        }

        // exit_code == -1 means this is the browser process (shouldn't happen for helper)
        eprintln!("Helper was invoked as browser process - this shouldn't happen");
        std::process::exit(1);
    }

    #[cfg(target_os = "windows")]
    {
        let cef_dir = resolve_windows_cef_dir();
        configure_windows_cef_dll_directory(&cef_dir);

        // Initialize CEF API
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

        let args = cef::args::Args::new();
        let mut app = browser::build_cef_app();

        let exit_code = cef::execute_process(
            Some(args.as_main_args()),
            Some(&mut app),
            std::ptr::null_mut(),
        );

        if exit_code >= 0 {
            std::process::exit(exit_code);
        }

        eprintln!("Helper was invoked as browser process - this shouldn't happen");
        std::process::exit(1);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        eprintln!("Helper is only needed on macOS and Windows");
        std::process::exit(1);
    }
}
