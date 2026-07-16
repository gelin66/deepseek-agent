//! Legacy `image_ocr` ToolSpec adapter around the tools-owned OCR backend.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::spec::{ToolCapability, ToolContext, ToolError, ToolOutcome, ToolSpec, required_str};

/// Tool implementing `image_ocr`. Runs a local OCR backend and returns the
/// extracted text on success.
pub struct ImageOcrTool;

#[async_trait]
impl ToolSpec for ImageOcrTool {
    fn name(&self) -> &'static str {
        "image_ocr"
    }

    fn description(&self) -> &'static str {
        "Extract text from an image (PNG, JPEG, or TIFF) via local OCR. On macOS this uses the built-in Vision framework; otherwise it uses local tesseract when available. Use this for screenshots, scanned receipts/whiteboards, image-only PDFs, or any visual that contains text the model needs to read. Returns the extracted text inline; no file is written."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the image file (relative to workspace or absolute). PNG / JPEG / TIFF supported."
                }
            },
            "required": ["path"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Sandboxable]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolOutcome, ToolError> {
        let path_str = required_str(&input, "path")?;
        let image_path = context.resolve_path(path_str)?;
        if !image_path.exists() {
            return Err(ToolError::execution_failed(format!(
                "image_ocr: source path does not exist: {}",
                image_path.display()
            )));
        }

        let text = codewhale_tools::ocr_image_path(&image_path)?;
        Ok(ToolOutcome::success(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Resolve the checked-in OCR fixture path. The image lives at
    /// `crates/tui/tests/fixtures/ocr_hello.png` (300x100 grayscale,
    /// "HELLO OCR" rendered in Helvetica) and is committed for the
    /// happy-path round-trip below.
    fn ocr_fixture_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_hello.png")
    }

    #[test]
    fn tool_metadata_marks_image_ocr_read_only_and_parallel() {
        let tool = ImageOcrTool;
        assert_eq!(tool.name(), "image_ocr");
        assert!(tool.supports_parallel());
        let caps = tool.capabilities();
        assert!(caps.contains(&ToolCapability::ReadOnly));
        assert!(!caps.contains(&ToolCapability::WritesFiles));
    }

    #[tokio::test]
    async fn image_ocr_rejects_missing_path() {
        let tmp = tempdir().expect("tempdir");
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let err = ImageOcrTool
            .execute(json!({"path": "definitely-not-here.png"}), &ctx)
            .await
            .expect_err("nonexistent path must reject before tesseract spawn");
        let msg = err.to_string();
        assert!(
            msg.contains("does not exist"),
            "error must call out missing path; got {msg}"
        );
    }

    #[tokio::test]
    async fn image_ocr_recovers_hello_from_fixture_image() {
        if !codewhale_tools::ocr_available() {
            // Tool wouldn't be registered without a local OCR backend — mirror
            // that here so the suite stays green on CI images that
            // intentionally omit OCR tooling.
            return;
        }
        let fixture = ocr_fixture_path();
        if !fixture.exists() {
            // Fixture not committed (sparse / shallow checkout). Skip
            // silently rather than failing the suite.
            return;
        }
        let tmp = tempdir().expect("tempdir");
        // Stage the fixture under the workspace so the path resolver
        // accepts the relative input — keeps the test independent of
        // the workspace boundary check inside `resolve_path`.
        let staged = tmp.path().join("ocr_hello.png");
        fs::copy(&fixture, &staged).unwrap();
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        let result = ImageOcrTool
            .execute(json!({"path": "ocr_hello.png"}), &ctx)
            .await
            .expect("execute");
        assert!(result.is_success());
        // Tesseract reliably recovers "HELLO OCR" from the rendered
        // PNG; allow either spacing variant.
        let normalised = result.content.to_uppercase();
        assert!(
            normalised.contains("HELLO") && normalised.contains("OCR"),
            "expected OCR to recover HELLO OCR; got {:?}",
            result.content
        );
    }
}
