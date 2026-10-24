//! Native image observations retain exact host bytes separately from prompt text.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;

pub(super) const INTERFACE: &str = "host-image-read-v1";
const KIND: &str = "image_read";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Capture {
    host_path: String,
    media_type: String,
    bytes: u64,
    sha256: String,
    input: provider::ImageInput,
}

impl Capture {
    fn metadata(&self) -> Value {
        json!({"host_path":self.host_path,"media_type":self.media_type,
            "bytes":self.bytes,"sha256":self.sha256,"detail":self.input.detail})
    }

    fn validate(&self) -> Result<()> {
        let prefix = format!("data:{};base64,", self.media_type);
        let encoded = self
            .input
            .image_url
            .strip_prefix(&prefix)
            .ok_or_else(|| held("retained image data URL does not match its media type"))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| held("retained image encoding is invalid"))?;
        require(
            bytes.len() as u64 == self.bytes && s::sha256_bytes(&bytes) == self.sha256,
            "retained image bytes do not match their capture",
        )
    }
}

pub(super) fn interface() -> Value {
    json!({"interface":INTERFACE,"operation":"image_read",
        "maximum_bytes":null,"maximum_images":null,
        "rule":"Read an existing regular host image with image_read {id,host_path,expected_sha256,detail}. Omit expected_sha256 or use null to capture the current bytes and receive their actual digest. Supply a known digest only when that exact revision matters; a mismatch supplies a failed read without pixels, not an automatic retry. detail is auto, low, high or original. Relative paths use the host command default working directory. PNG, JPEG, WebP and GIF signatures are recognized; the provider determines whether the captured image is decodable and supported (GIF support is for non-animated images). No resizing, conversion or byte/count quota is applied. A command-produced image needs an observed command result and a fresh decision: image reads happen before proposed commands execute. SVG, PDF and other sources can be rendered to a supported raster with host commands first. The capture is an immutable observation, not authored asset maintenance. Its metadata alone does not supply pixels. On the next request CONTROL.image_inputs maps ordered provider image content parts to exact captures and hashes; inspect those images before making visual claims. A failed read supplies its actual error without an image. Reads do not establish authorship, correctness or reviewer approval. After consumption, an exact history_record_read of the original image_read observation can supply the retained bytes again without reopening the host path. Choose continuation and any useful further reading yourself."})
}

fn capture(
    prepared: &Prepared,
    path: &str,
    expected: Option<&str>,
    detail: provider::ImageDetail,
) -> Result<Capture> {
    let interface = prepared
        .host_command_interface
        .as_ref()
        .ok_or_else(|| held("image_read requires the ordinary host interface"))?;
    let requested = Path::new(path);
    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        Path::new(&interface.default_working_directory).join(requested)
    };
    // Opening a FIFO must not block before the regular-file check.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&resolved)?;
    require(
        file.metadata()?.is_file(),
        "image_read requires a regular file",
    )?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let sha256 = s::sha256_bytes(&bytes);
    require(
        expected.is_none_or(|expected| expected == sha256),
        "image_read expected_sha256 does not match the bytes read",
    )?;
    // This is format identification, not a successful image decode or visual
    // assessment. Exact bytes go to the provider; decode failures remain real.
    let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else {
        return Err(held("image_read requires a PNG, JPEG, WebP or GIF signature; render other formats with host commands"));
    };
    Ok(Capture {
        host_path: resolved.to_string_lossy().into_owned(),
        media_type: media_type.into(),
        bytes: bytes.len() as u64,
        sha256,
        input: provider::ImageInput {
            image_url: format!("data:{media_type};base64,{}", STANDARD.encode(&bytes)),
            detail,
        },
    })
}

pub(super) fn validate_interface(prepared: &Prepared) -> Result<()> {
    require(
        prepared.request.mode == Mode::Ordinary
            && prepared.host_command_interface.is_some()
            && request_context::decode(prepared)?.control["image_read"]["interface"] == INTERFACE,
        "image_read was not offered in this admission",
    )
}

pub(super) fn read(
    state: &mut State,
    prepared: &Prepared,
    id: &str,
    path: &str,
    expected: Option<&str>,
    detail: provider::ImageDetail,
    now: i64,
) -> Result<()> {
    validate_interface(prepared)?;
    let result = capture(prepared, path, expected, detail);
    let payload = match &result {
        Ok(capture) => json!({"status":"captured","capture":capture.metadata(),
            "image_supplied":false,"meaning":"Exact bytes retained; see the actual request's image_inputs for image body supply. Capture does not establish visual assessment."}),
        Err(error) => {
            json!({"status":"unavailable","host_path":path,"error":error.to_string(),"image_supplied":false})
        }
    };
    add_observation(
        state,
        prepared,
        id,
        KIND,
        payload,
        Vec::new(),
        Vec::new(),
        now,
    )?;
    if let Ok(capture) = result {
        let observation = observation_id(&prepared.request.id, KIND, id);
        require(
            !state.image_captures.contains_key(&observation),
            "image capture identity conflict",
        )?;
        state.image_captures.insert(observation, Arc::new(capture));
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Supply {
    pub images: Vec<provider::ImageInput>,
    pub metadata: Vec<Value>,
    supplied: BTreeSet<String>,
}

impl Supply {
    /// Called only after the native observation or exact historical record has
    /// passed its current projection checks. Do not search arbitrary payload JSON
    /// for image-like keys or treat an index/reference as a supplied body.
    pub fn include(
        &mut self,
        state: &State,
        observation: &Observation,
        value: &mut Value,
    ) -> Result<()> {
        let (original, projected) = if observation.kind == KIND {
            (observation, value)
        } else if observation.kind == "history_record_read"
            && value["payload"]["body_supplied"] == true
        {
            let reference: p::HistoryRecordReference =
                serde_json::from_value(observation.payload["reference"].clone())?;
            let p::HistoryRecordReference::Observation { observation_id, .. } = reference else {
                return Ok(());
            };
            let original = state
                .observations
                .get(&observation_id)
                .ok_or_else(|| held("supplied historical image observation is missing"))?;
            (
                original,
                &mut value["payload"]["record"]["historical_record"],
            )
        } else {
            return Ok(());
        };
        if original.kind != KIND || original.payload["status"] != "captured" {
            return Ok(());
        }
        let capture = state
            .image_captures
            .get(&original.id)
            .ok_or_else(|| held("retained image capture is missing"))?;
        require(
            original.payload["capture"] == capture.metadata(),
            "image capture metadata mismatch",
        )?;
        if self.supplied.insert(original.id.clone()) {
            capture.validate()?;
            let mut metadata = capture.metadata();
            metadata["image_index"] = json!(self.images.len());
            metadata["observation_id"] = json!(original.id);
            metadata["capturing_invocation"] = json!(original.invocation);
            metadata["history_reference"] = serde_json::to_value(
                history::records::observation_identity(original)
                    .ok_or_else(|| held("image capture history identity unavailable"))?
                    .0,
            )?;
            metadata["body_supplied"] = json!(true);
            self.metadata.push(metadata);
            self.images.push(capture.input.clone());
        }
        projected["payload"]["image_supplied"] = json!(true);
        Ok(())
    }
}
