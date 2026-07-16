//! Local OCR backends shared by every `read_file` caller.
//!
//! macOS uses the built-in Vision framework first. Other platforms, and
//! macOS when Vision cannot be used, fall back to a locally installed
//! Tesseract binary.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::ToolError;

/// Return whether at least one local OCR backend is available.
#[must_use]
pub fn ocr_available() -> bool {
    native_ocr_available() || resolve_tesseract().is_some()
}

/// Extract text from an image with the best available local OCR backend.
pub fn ocr_image_path(image_path: &Path) -> Result<String, ToolError> {
    if !image_path.exists() {
        return Err(ToolError::execution_failed(format!(
            "image_ocr: source path does not exist: {}",
            image_path.display()
        )));
    }

    let native_error = match try_native_ocr(image_path) {
        Ok(Some(text)) => return Ok(text),
        Ok(None) => None,
        Err(error) => Some(error),
    };

    if let Some(tesseract) = resolve_tesseract() {
        return ocr_with_tesseract(&tesseract, image_path);
    }

    if let Some(error) = native_error {
        return Err(error);
    }

    Err(ToolError::execution_failed(
        "image_ocr: no local OCR backend is available. On macOS, update to a version with the Vision framework; on Linux/Windows install tesseract and restart codewhale.",
    ))
}

/// Resolve Tesseract once per process after verifying that it can run.
#[must_use]
pub fn resolve_tesseract() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut command = Command::new("tesseract");
            command
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            suppress_console_window(&mut command);

            if matches!(command.status(), Ok(status) if status.success()) {
                tracing::info!(
                    target: "tool_dependencies",
                    "Resolved tesseract binary for image_ocr",
                );
                Some("tesseract".to_string())
            } else {
                tracing::warn!(
                    target: "tool_dependencies",
                    "tesseract binary not found; image_ocr will rely on native OCR if available",
                );
                None
            }
        })
        .clone()
}

fn ocr_with_tesseract(tesseract: &str, image_path: &Path) -> Result<String, ToolError> {
    // `tesseract <image> -` writes recognized text to stdout without creating
    // a sidecar file.
    let mut command = Command::new(tesseract);
    command
        .arg(image_path)
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let output = command.output().map_err(|error| {
        ToolError::execution_failed(format!("failed to launch tesseract: {error}"))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ToolError::execution_failed(format!(
            "tesseract failed (exit {:?}): {stderr}",
            output.status.code()
        )));
    }

    // Some Tesseract builds append a form-feed. Trimming trailing whitespace
    // preserves the existing inline result contract.
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

#[cfg(target_os = "windows")]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn suppress_console_window(_command: &mut Command) {}

#[cfg(target_os = "macos")]
fn native_ocr_available() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
fn native_ocr_available() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
fn try_native_ocr(_image_path: &Path) -> Result<Option<String>, ToolError> {
    Ok(None)
}

#[cfg(target_os = "macos")]
#[link(name = "Vision", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
fn try_native_ocr(image_path: &Path) -> Result<Option<String>, ToolError> {
    macos_vision::recognize_text(image_path).map(Some)
}

#[cfg(target_os = "macos")]
mod macos_vision {
    use super::*;
    use objc2::msg_send;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSArray, NSDictionary, NSError, NSString, NSURL};
    use std::ptr;

    pub(super) fn recognize_text(image_path: &Path) -> Result<String, ToolError> {
        autoreleasepool(|_| recognize_text_inner(image_path))
    }

    fn recognize_text_inner(image_path: &Path) -> Result<String, ToolError> {
        let url = NSURL::from_file_path(image_path).ok_or_else(|| {
            ToolError::execution_failed(format!(
                "image_ocr: failed to build file URL for {}",
                image_path.display()
            ))
        })?;

        let request_class = AnyClass::get(c"VNRecognizeTextRequest").ok_or_else(|| {
            ToolError::execution_failed("image_ocr: macOS Vision text request is unavailable")
        })?;
        let handler_class = AnyClass::get(c"VNImageRequestHandler").ok_or_else(|| {
            ToolError::execution_failed("image_ocr: macOS Vision image handler is unavailable")
        })?;

        let request = new_object(request_class, "VNRecognizeTextRequest")?;
        // VNRequestTextRecognitionLevelAccurate is 0.
        unsafe {
            let _: () = msg_send![&*request, setRecognitionLevel: 0usize];
            let _: () = msg_send![&*request, setUsesLanguageCorrection: true];
        }

        let requests = NSArray::from_slice(&[&*request]);
        let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();

        let handler_alloc = alloc_object(handler_class, "VNImageRequestHandler")?;
        let handler_raw: *mut AnyObject =
            unsafe { msg_send![handler_alloc, initWithURL: &*url, options: &*options] };
        let handler = unsafe { Retained::from_raw(handler_raw) }.ok_or_else(|| {
            ToolError::execution_failed("image_ocr: failed to initialize Vision image handler")
        })?;

        let mut error: *mut NSError = ptr::null_mut();
        let ok: bool =
            unsafe { msg_send![&*handler, performRequests: &*requests, error: &mut error] };
        if !ok {
            return Err(ToolError::execution_failed(format!(
                "image_ocr: macOS Vision failed{}",
                vision_error_suffix(error)
            )));
        }

        collect_recognized_text(&request)
    }

    fn new_object(class: &AnyClass, label: &str) -> Result<Retained<AnyObject>, ToolError> {
        let raw: *mut AnyObject = unsafe { msg_send![class, new] };
        unsafe { Retained::from_raw(raw) }.ok_or_else(|| {
            ToolError::execution_failed(format!("image_ocr: failed to create {label}"))
        })
    }

    fn alloc_object(class: &AnyClass, label: &str) -> Result<*mut AnyObject, ToolError> {
        let raw: *mut AnyObject = unsafe { msg_send![class, alloc] };
        if raw.is_null() {
            Err(ToolError::execution_failed(format!(
                "image_ocr: failed to allocate {label}"
            )))
        } else {
            Ok(raw)
        }
    }

    fn collect_recognized_text(request: &AnyObject) -> Result<String, ToolError> {
        let results: *mut AnyObject = unsafe { msg_send![request, results] };
        if results.is_null() {
            return Ok(String::new());
        }

        let count: usize = unsafe { msg_send![results, count] };
        let mut lines = Vec::new();
        for index in 0..count {
            let observation: *mut AnyObject = unsafe { msg_send![results, objectAtIndex: index] };
            if observation.is_null() {
                continue;
            }
            let candidates: *mut AnyObject =
                unsafe { msg_send![observation, topCandidates: 1usize] };
            if candidates.is_null() {
                continue;
            }
            let candidate_count: usize = unsafe { msg_send![candidates, count] };
            if candidate_count == 0 {
                continue;
            }
            let candidate: *mut AnyObject = unsafe { msg_send![candidates, objectAtIndex: 0usize] };
            if candidate.is_null() {
                continue;
            }
            let text: *mut NSString = unsafe { msg_send![candidate, string] };
            if text.is_null() {
                continue;
            }
            let line = unsafe { &*text }.to_string();
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }

        Ok(lines.join("\n"))
    }

    fn vision_error_suffix(error: *mut NSError) -> String {
        if error.is_null() {
            return String::new();
        }
        let description: *mut NSString = unsafe { msg_send![error, localizedDescription] };
        if description.is_null() {
            String::new()
        } else {
            format!(": {}", unsafe { &*description })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_image_is_rejected_before_backend_launch() {
        let workspace = tempdir().expect("workspace");
        let missing = workspace.path().join("missing.png");
        let error = ocr_image_path(&missing).expect_err("missing image");
        assert!(error.to_string().contains("source path does not exist"));
    }

    #[test]
    fn missing_tesseract_process_is_a_typed_tool_error() {
        let workspace = tempdir().expect("workspace");
        let image = workspace.path().join("image.png");
        std::fs::write(&image, b"not an image").expect("fixture");
        let error =
            ocr_with_tesseract("codewhale-missing-tesseract", &image).expect_err("missing binary");
        assert!(error.to_string().contains("failed to launch tesseract"));
    }
}
