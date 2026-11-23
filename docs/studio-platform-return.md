# Continuing a saved Studio brief on an OpenAI API key

On November 23, 2026, native Studio switched Anya and Bo from Codex login to an operator-selected OpenAI Platform key file after both had been invited. The switch worked. **The provider then returned `insufficient_quota` on both first calls.** No persona decision, design artifact or manufacturing handoff resulted from this Platform test.

**What the operator did.** The workspace started with an empty working directory and two general character profiles. The requester entered a brief for an editable 120 × 60 mm shop sign with two 4 mm mounting holes, left design choices to the invitees, and selected **Save without starting**. In the reopened workspace, **Workspace settings → Model connection** showed Anya on `gpt-6.1-sol` / medium and Bo on the same model / high. The operator chose **OpenAI API key → Use key file**, using the path previously selected in that workspace.

[The connection choices and the two personas’ settings](assets/studio-platform-return-connection.jpg) · [Both provider pauses and their receipt links](assets/studio-platform-return-quota.jpg)

**What was verified.** Event 4 changed only the workspace credential configuration and the two persona provider settings. At that boundary there were zero admitted calls or effects; the exact brief, both invitations and their participation records were unchanged. Their provider bindings matched the original Platform file binding. Selecting **Run workspace** then made two direct Platform Responses requests. HTTP 200 opened the streams; event 3 of each stream reported error **type** `insufficient_quota`. Neither stream completed or reported a model result. The requested model should not be mistaken for a model that successfully ran.

| Persona | Requested model and effort | Transport | Failed receipt SHA-256 |
|---|---|---|---|
| Anya | `gpt-6.1-sol`, medium | `platform_responses_v6` | `91a7d9ad9d49458dc7d847beebafce5c4e0d35180dcbf62811c02b8738dcc41f` |
| Bo | `gpt-6.1-sol`, high | `platform_responses_v6` | `6efab4948df71b405546ecf7c6d8e566f22ac02ff952ca4647a7a398f6e3d9f4` |

The activity was `work-13d3d4dda06e6530d925faf3718b92cd`. Original brief SHA-256: `9ce4cdfa2a741789b5a9daf4b5eaf22250da7eddba4cb9b83a0da22079c08869`. No retry was scheduled or OAuth fallback performed. The idle shop run was finished with both invitations and pauses saved. Once Platform quota is available, the requester can enable both personas and run the same activity. [Verified observations and read-only replay](https://github.com/ai-personas/ai-personas-runtime/releases/tag/studio-platform-return-20261123)

**Existing-character check.** A separate, execution-free test used a disposable copy of Linden and Pax's Fieldwork workspace. After switching from OAuth back to a private key file, both retained their epoch-one numerical packages. All 42 previously accepted decisions and terminal provider outputs, four issued editions, four activities, participation choices and the operator pause were preserved. Reapplying the same binding wrote no event or revision. Replacing the test key at the same path explicitly rebound both personas when applied; the test secrets were absent from the database objects. Active-run ownership, an unsettled invocation or tool, a missing or nonprivate file, an invalid credential document and a path resolving inside the shared project each left the connection unchanged.

The [four-person ATX engineering results](pc-power-studio.md#engineering-review-15-a-better-standby-path) are from a separate continuing OAuth activity. They are not evidence of a successful Platform call.
