//! Commit a derived document or page image in the same transaction as its receipt.
use super::{control_observation, failure, Operation, Result, Store};
use crate::neural::{html_print, provider_evidence, store, tools::RenderPdf, workspace};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;

pub(super) enum File {
    PageImage(Vec<u8>),
    Pdf(Vec<u8>),
}

impl File {
    pub(super) fn retain(self, db: &Connection, result: &mut Value) -> Result<()> {
        let digest = match self {
            Self::Pdf(bytes) => store::put_bytes(db, "application/pdf", &bytes)?,
            Self::PageImage(bytes) => {
                let digest = store::put_bytes(db, "image/png", &bytes)?;
                result["image_blob"] = json!(digest);
                digest
            }
        };
        result["reference"] = json!(format!("object:{digest}"));
        Ok(())
    }
}

pub(super) async fn execute(
    root: &Path,
    work: &str,
    actor: &str,
    op: Operation,
) -> Result<(Value, Option<File>)> {
    let root = root.to_owned();
    let work = work.to_owned();
    let actor = actor.to_owned();
    tokio::task::spawn_blocking(move || {
        let store = Store::open(&root)?;
        let stopped = || control_observation(&store, &work, &actor).is_some();
        match op.tool.as_str() {
            "document.print_html" => {
                let input = serde_json::from_value(op.input)?;
                let (result, pdf) = html_print::print_retained(&store.db, &input, stopped);
                Ok((result, pdf.map(File::Pdf)))
            }
            "document.render_pdf" => render(&store.db, serde_json::from_value(op.input)?, stopped),
            _ => Err(failure("Unknown document effect")),
        }
    })
    .await?
}

fn render(
    db: &Connection,
    input: RenderPdf,
    stopped: impl FnMut() -> bool,
) -> Result<(Value, Option<File>)> {
    let mut result = json!({
        "success": false,
        "source": input.reference,
        "page": input.page,
        "automatic_retry": false
    });
    let source = workspace::byte_digest(db, &input.reference).and_then(|digest| {
        let bytes = provider_evidence::resource_bytes(db, &digest)?;
        Ok((digest, bytes))
    });
    let (digest, bytes) = match source {
        Ok(source) => source,
        Err(error) => {
            result["error"] = json!(error.to_string());
            return Ok((result, None));
        }
    };
    result["source_bytes"] = json!(format!("object:{digest}"));
    result["source_byte_count"] = json!(bytes.len());
    let rendered = crate::pdf_preview::render_with_control(&bytes, input.page, stopped);
    match rendered {
        Ok(page) => {
            let receipt = json!({
                "success": true,
                "source": input.reference,
                "source_bytes": format!("object:{digest}"),
                "source_byte_count": bytes.len(),
                "page": input.page,
                "page_count": page.page_count,
                "renderer": "Poppler pdftoppm",
                "width": page.width,
                "height": page.height,
                "mime": "image/png",
                "bytes": page.png.len()
            });
            Ok((receipt, Some(File::PageImage(page.png))))
        }
        Err(error) => {
            result["error"] = json!(error.to_string());
            if let Some(count) = error.page_count {
                result["page_count"] = json!(count);
            }
            Ok((result, None))
        }
    }
}
