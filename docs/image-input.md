# Visual inspection of host images

An ordinary persona can choose `image_read` to capture an existing host image
and receive its pixels in the next model request:

```json
{
  "kind": "image_read",
  "id": "inspect-rendered-plot",
  "host_path": "/workspace/output/plot.png",
  "expected_sha256": null,
  "detail": "auto"
}
```

`CONTROL.image_read` supplies this interface without a guide, grant or approval.
Relative paths use the host command's displayed default working directory.
`expected_sha256`, when present, checks the raw bytes actually read. `detail` is
the persona's provider setting: `auto`, `low`, `high` or `original`. The exact
requested model and effort remain unchanged; actual provider rejection does not
trigger a different model, access route or retry.

Omit the expected digest or use `null` to capture the current image and receive
its actual digest. A previously observed digest is useful when that exact
revision matters; a mismatch supplies a failed-read observation without pixels.
The persona chooses whether to inspect the changed file, retrieve an earlier
immutable capture, or take another action. A new invocation does not itself
require a new image capture.

The runtime recognizes PNG, JPEG, WebP and GIF file signatures and retains the
complete bytes. It does not decode, resize, crop or convert the image, and adds
no count or byte quota. Signature recognition is not proof of a valid image.
The provider determines whether it can decode and accept the input; the public
Responses format supports non-animated GIFs. SVGs, PDFs and other sources can be
rendered with host commands first. A command-generated file needs an observed
command result and a fresh decision: image reads occur before proposed commands
execute. Opening a directory, device or pipe does not capture an image.

The capture is an observation, so it does not require an asset-maintenance
change or claim authorship of the image. An unavailable read records its actual
error and supplies no image. A successful capture records its host path, media
type, raw byte count, SHA-256 and requested detail. The immutable capture is
retained privately alongside the native observation. It does not change the
host file or make a copy into the persona's authored text collection.

On the next request, `CONTROL.image_inputs.images` identifies the ordered image
content parts by zero-based `image_index`, capture observation and invocation,
path, bytes, hash and detail. The text prompt and image parts are supplied
together. The projected observation then says `image_supplied: true`; its
original capture receipt remains unchanged and does not itself claim supply.
Seeing metadata, reading SVG source and receiving a raster image are different
events. A persona's visual judgment remains an attributed interpretation, not
proof of numerical correctness, physical measurement or independent approval.

Once an accepted decision consumes the pending observation, later inputs do
not automatically repeat the image. The persona can select useful captures in
`next_assets.images`, alongside any selected text:

```json
{
  "kind": "replace_from_input",
  "reason": "Keep the captured view alongside the next result.",
  "handles": [],
  "images": [
    {
      "kind": "from_input_history",
      "handle": "call-00000012:h1",
      "lifetime": "work"
    }
  ]
}
```

Copy an actual current `CONTROL.input_history_choices` handle that names the
original successful `image_read` observation. Alternatively, use
`{"kind":"exact","reference":<original history_reference>,"lifetime":"work"}`.
Known exact captures from other works or personas remain available with their
original attribution. A historical-read receipt is not the original capture.

Every `next_assets` replacement chooses the complete text and image set.
Omitting `images` from a replacement clears earlier image choices. Use empty
`handles` or `references` to select images alone. `preserve` retains both kinds
without renewing their lifetimes; `clear` removes both. The default `work`
lifetime lasts in this work until replacement or clearing; `next_call` lasts
only through the next admission. Other work does not inherit this selection.
An accepted character change permits only preservation or reduction of whole
surviving image choices, without renewing a lifetime.

`CONTROL.selection_state.images` records the choices and their lifetimes.
`CONTROL.selected_image_context` supplies their original attributed observations
and current required qualifications. Actual pixel supply remains recorded in
`CONTROL.image_inputs`. A capture selected and also pending in the same input
is sent once. Selection reuses immutable captured bytes and detail; it does not
reopen the host path, create another read receipt, mutate an asset or establish
that the current file still matches. A new replacement must resolve coherently;
an unavailable preserved capture receives an explicit disposition so an ordinary
decision can repair its selection. No image catalogue, quota or automatic
selection is introduced.

The persona can also retrieve the exact original
`image_read` observation through `history_record_read` or its current input
handle. The supplied image metadata includes that exact `history_reference`,
also offered through `CONTROL.input_history_choices`. A successfully projected
full historical capture supplies the same
retained pixels again without reopening the host path. History queries and
metadata alone supply no image. No reread creates a new host observation.

The direct provider serializes each capture as an `input_image` with a Base64
data URL and the chosen detail, following the
[Responses image-input format](https://developers.openai.com/api/docs/guides/images-vision).
The native `Prepared.images` retains those exact parts. `prompt_sha256` and
`prompt_bytes` describe text only; the ordered `RequestAudit.image_inputs`
records each data URL's hash, byte count, detail and `responses-input-image-v1`
format. `request_sha256` and `request_bytes` bind the entire serialized provider
body, including images. Response-only recovery reconstructs that same body.
No remote image URL fetch or provider-hosted tool is introduced. Text-only
requests retain their existing wire serialization and omit the image audit.
