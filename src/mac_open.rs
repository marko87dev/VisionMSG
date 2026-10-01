//! Receives Finder documents through winit's macOS application delegate.

use objc2::runtime::{AnyClass, AnyObject, Imp, Method, Sel};
use objc2::sel;
use objc2_foundation::NSString;
use std::ffi::c_char;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static OPENED_FILES: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
type SetDelegate = extern "C-unwind" fn(&AnyObject, Sel, Option<&AnyObject>);
static ORIGINAL_SET_DELEGATE: OnceLock<SetDelegate> = OnceLock::new();

#[link(name = "objc")]
unsafe extern "C" {
    fn class_addMethod(
        class: *const AnyClass,
        selector: Sel,
        implementation: Imp,
        types: *const c_char,
    ) -> bool;
}

fn queue() -> &'static Mutex<Vec<PathBuf>> {
    OPENED_FILES.get_or_init(|| Mutex::new(Vec::new()))
}

extern "C-unwind" fn open_file(
    _delegate: &AnyObject,
    _selector: Sel,
    _app: &AnyObject,
    filename: &NSString,
) -> bool {
    let path = PathBuf::from(filename.to_string());
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("msg"))
    {
        return false;
    }
    if let Ok(mut files) = queue().lock() {
        files.push(path);
        true
    } else {
        false
    }
}

extern "C-unwind" fn set_delegate(app: &AnyObject, selector: Sel, delegate: Option<&AnyObject>) {
    if let Some(delegate) = delegate {
        let class = delegate.class();
        if class.name().to_bytes() == b"WinitApplicationDelegate" {
            let method: Imp = unsafe {
                std::mem::transmute::<extern "C-unwind" fn(_, _, _, _) -> _, Imp>(open_file)
            };
            // Objective-C encoding: BOOL, self, selector, NSApplication, NSString.
            unsafe {
                class_addMethod(
                    class,
                    sel!(application:openFile:),
                    method,
                    c"c@:@@".as_ptr(),
                )
            };
        }
    }
    ORIGINAL_SET_DELEGATE
        .get()
        .expect("Original setDelegate: missing")(app, selector, delegate);
}

/// Install before eframe creates the winit event loop. When winit installs its
/// delegate, add the document callback before macOS delivers startup files.
pub fn install() {
    let class = objc2::class!(NSApplication);
    let method: &Method = class
        .instance_method(sel!(setDelegate:))
        .expect("NSApplication.setDelegate:");
    let replacement: Imp = unsafe { std::mem::transmute::<SetDelegate, Imp>(set_delegate) };
    let original = unsafe { method.set_implementation(replacement) };
    ORIGINAL_SET_DELEGATE
        .set(unsafe { std::mem::transmute::<Imp, SetDelegate>(original) })
        .expect("Open-file hook installed twice");
}

pub fn take_opened_files() -> Vec<PathBuf> {
    queue()
        .lock()
        .map(|mut files| std::mem::take(&mut *files))
        .unwrap_or_default()
}

pub fn queue_command_line_files() {
    if let Ok(mut files) = queue().lock() {
        files.extend(
            std::env::args_os()
                .skip(1)
                .map(PathBuf::from)
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("msg"))
                }),
        );
    }
}
