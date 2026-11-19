//! Keep a derived image in memory until its outbox result is committed with it.
use super::{control_observation, tools::RenderPdf, Result, Store};
use crate::neural::{provider_evidence, workspace};
use serde_json::{json, Value};
use std::path::Path;

pub(super) async fn render(
    root: &Path,
    work: &str,
    actor: &str,
    input: RenderPdf,
) -> Result<(Value, Option<Vec<u8>>)> {
    let root = root.to_owned();
    let work = work.to_owned();
    let actor = actor.to_owned();
    tokio::task::spawn_blocking(move || {
        let store = Store::open(&root)?;
        let mut result = json!({
            "success": false,
            "source": input.reference,
            "page": input.page,
            "automatic_retry": false
        });
        let source = workspace::byte_digest(&store.db, &input.reference).and_then(|digest| {
            let bytes = provider_evidence::resource_bytes(&store.db, &digest)?;
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
        let rendered = crate::pdf_preview::render_with_control(&bytes, input.page, || {
            control_observation(&store, &work, &actor).is_some()
        });
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
                Ok((receipt, Some(page.png)))
            }
            Err(error) => {
                result["error"] = json!(error.to_string());
                if let Some(count) = error.page_count {
                    result["page_count"] = json!(count);
                }
                Ok((result, None))
            }
        }
    })
    .await?
}
