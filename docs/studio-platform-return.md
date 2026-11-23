# Continuing a saved Studio brief on an OpenAI API key

On November 23, 2026, native Studio switched Anya and Bo from Codex login to an operator-selected OpenAI Platform key file after both had been invited. The switch worked. **The provider then returned `insufficient_quota` on both first calls.** No persona decision, design artifact or manufacturing handoff resulted from this Platform test.

**What the operator did.** The workspace started with an empty working directory and two general character profiles. The requester entered a brief for an editable 120 × 60 mm shop sign with two 4 mm mounting holes, left design choices to the invitees, and selected **Save without starting**. In the reopened workspace, **Workspace settings → Model connection** showed Anya on `gpt-6.1-sol` / medium and Bo on the same model / high. The operator chose **OpenAI API key → Use key file**, using the path previously selected in that workspace.

[The connection choices and the two personas’ settings](assets/studio-platform-return-connection.jpg) · [Both provider pauses and their receipt links](assets/studio-platform-return-quota.jpg)

**What was verified.** Event 4 changed only the workspace credential configuration and the two persona provider settings. At that boundary there were zero admitted calls or effects; the exact brief, both invitations and their participation records were unchanged. Their provider bindings matched the original Platform file binding. Selecting **Run workspace** then made two direct Platform Responses requests. HTTP 200 opened the streams; event 3 of each stream reported error **type** `insufficient_quota`. Neither stream completed or reported a model result. The requested model should not be mistaken for a model that successfully ran.

| Persona | Requested model and effort | Transport | Failed receipt SHA-256 |
|---|---|---|---|
| Anya | `gpt-6.1-sol`, medium | `platform_responses_v6` | `91a7d9ad9d49458dc7d847beebafce5c4e0d35180dcbf62811c02b8738dcc41f` |
| Bo | `gpt-6.1-sol`, high | `platform_responses_v6` | `6efab4948df71b405546ecf7c6d8e566f22ac02ff952ca4647a7a398f6e3d9f4` |

The activity was `work-13d3d4dda06e6530d925faf3718b92cd`. Original brief SHA-256: `9ce4cdfa2a741789b5a9daf4b5eaf22250da7eddba4cb9b83a0da22079c08869`. No retry was scheduled or automatic OAuth fallback performed. The idle Platform run was finished with both invitations and pauses saved. [Verified Platform observations and read-only replay](https://github.com/ai-personas/ai-personas-runtime/releases/tag/studio-platform-return-20261123)

**Existing-character check.** A separate, execution-free test used a disposable copy of Linden and Pax's Fieldwork workspace. After switching from OAuth back to a private key file, both retained their epoch-one numerical packages. All 42 previously accepted decisions and terminal provider outputs, four issued editions, four activities, participation choices and the operator pause were preserved. Reapplying the same binding wrote no event or revision. Replacing the test key at the same path explicitly rebound both personas when applied; the test secrets were absent from the database objects. Active-run ownership, an unsettled invocation or tool, a missing or nonprivate file, an invalid credential document and a path resolving inside the shared project each left the connection unchanged.

The [four-person ATX engineering results](pc-power-studio.md#engineering-review-15-a-better-standby-path) are from a separate continuing OAuth activity. They are not evidence of a successful Platform call.

## Resuming the same invitation with Codex OAuth

At the requester's direction, the operator chose **Use Codex login**, enabled Anya and Bo, then selected **Run workspace**. Neither persona needed a new invitation. All six opening OAuth decisions completed, were accepted, and matched the retained final provider output byte for byte. The provider reported `gpt-6.1-sol` in every call: three at medium effort for Anya and three at high effort for Bo. The runtime used the direct `traditional_chatgpt_responses_v6` connection; Codex supplied authentication.

With their own numerical characters active, Anya accepted the original sign brief at 20:04:15 UTC and Bo accepted at 20:04:27 UTC. Each received the original unamended brief and the receipt for their own earlier failed Platform attempt. Anya chose to produce the editable artwork and manufacturing files; Bo chose to verify the geometry and shop handoff. The operator supplied no design directions. [Their first exchange in native Studio](assets/studio-shop-codex-resumed.jpg) · [OAuth receipts and read-only replay](https://github.com/ai-personas/ai-personas-runtime/releases/tag/studio-shop-codex-resumption-20261123)

## The issued laser-shop sign

**[Download the ZIP to forward to the shop](https://github.com/ai-personas/ai-personas-runtime/releases/download/studio-shop-sign-handoff-20261123/Tool-check-in_shop-package.zip)** · [View the issued PDF proof](https://github.com/ai-personas/ai-personas-runtime/releases/download/studio-shop-sign-handoff-20261123/Tool-check-in_PROOF.pdf)

Anya issued the seven-file ZIP at 20:08:18 UTC. It contains the live-text SVG, the outlined production SVG, a 1:1 PDF proof, shop instructions, and the font with its license files. Both Anya and Bo then independently opened the issued package in Studio. After Bo reported his findings, Anya sent the requester the unchanged ZIP at 20:10:09 UTC. Both chose to stop. [Completed handoff in Studio](assets/studio-shop-sign-issued.jpg)

| Receiving check on the issued files | Result |
|---|---|
| Will the cut artwork import at its intended scale? | MeerK40t imported one 120 × 60 mm rounded perimeter (R3) and two Ø4 mm holes, with centers at (8, 30) and (112, 30) mm. They are 104 mm apart. |
| Can a shop distinguish cutting from engraving? | Following the README, the receiver mapped the three red contours to through-cut and the twelve filled black letter paths to raster engraving. No artwork was redrawn or resized. |
| Is the wording both editable and reproducible? | The editable SVG uses live text, **Tool check-in**. Chrome loaded the embedded bold font offline and a temporary text edit rendered. Every production path and glyph position matched the bundled font, including spacing at the word break. The PDF and CAM object bounds agreed within 0.005 mm. |
| Is the specified stock identifiable? | The issuer requested nominal 3 mm white-face/black-core laser-engravable acrylic. [Rowmark LaserMax](https://www.rowmark.com/products/laser-max) lists `LM942204`, two-ply ⅛″ (3.175 mm); [Johnson Plastics Plus](https://www.jpplus.com/rowmark-lasermax-engraving-plastic?sku=LM942204-QTR) lists the matching sheet. This is a receiver-found example. The shop still selects suitable stock and confirms tolerances. |

The original Studio dispatch, the file inside its printable handoff and the downloadable ZIP have the same SHA-256: `c749184a5b726370d94618c573f0e20dffe88dea86c9729d0f76acb8a5dd7e9b`. Through Anya’s final message, all **29 accepted OAuth decisions** matched the retained provider outputs (16 Anya, 13 Bo); the two failed Platform calls are separate. The personas exchanged 15 messages and received no requester design follow-up. This demonstrates a digital handoff a shop can quote without requester edits. No shop quote, order, fabrication or physical measurement took place. [Receiving checks, scripts and receipts](https://github.com/ai-personas/ai-personas-runtime/releases/tag/studio-shop-sign-handoff-20261123)

## Opening a handoff from the persona’s reply

Anya’s final message included a Markdown link to the exact asset. Studio previously displayed its address as plain text. From the rebuilt [Society record](assets/studio-reply-file-link.jpg), the requester selected **File · v1 → Tool-check-in_shop-package.zip** directly from reply 103. The archive viewer showed seven files. **Save file…** wrote the same 167,071 bytes and SHA-256 as the original shop dispatch, with no new model call or journal event.

In a separate offline copy with two different revisions under the same filename, the reader saved both the original 167,071-byte ZIP and a distinct 199-byte comparison ZIP. Both exports matched the selected revisions. A package link opened the original dispatch, and a link to an absent revision reported the missing edition instead of substituting the current file. The comparison file and message were synthetic and never entered the personas’ activity.

## Receiving-sheet follow-up on the OpenAI API key

The requester invited Anya to produce a new, editable one-page receiving sheet for ten of the previously designed signs. The operator selected the workspace’s existing Platform key file; both original sign participations remained stopped, and both epoch-one numerical characters were preserved. The new activity is `work-8fe8abb0ad6f27afc49b527b7dfd9c7a`; its invitation was event 105.

At **21:02:36 UTC on November 23** Anya’s first ordinary call requested `gpt-6.1-sol` / medium over `platform_responses_v6`. The provider opened the stream (HTTP 200), then reported `insufficient_quota` on stream event 3. The stream did not complete or report a model result. Failed receipt SHA-256: `7d600a13cb467af2e52b088355ce056b83dad84a773285bf2e05a77857b61ad9`. **No tool ran and no receiving sheet was issued.** The new invitation is saved. After restoring Platform quota or selecting a funded key, enable Anya and run the workspace to continue. [Native Studio result](assets/studio-receiving-sheet-quota.jpg) · [Observations and replay](https://github.com/ai-personas/ai-personas-runtime/releases/tag/studio-reply-editions-20261123)
