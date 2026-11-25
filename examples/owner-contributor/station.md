# Prepare the two Studio desks (facilitator)

The station is **private data**, not a directory supplied by a Git checkout. Use a local Linux desktop with Python 3.9+ and readable `/proc` for your own processes. Use a separate, settled workspace, a new empty project and a host policy appropriate to the trial. The two windows share the workspace, not their drafts. Never put credentials, archives, study notes or the participants’ task cards in the project.

## Lay out the station

Set `OWNER_CONTRIBUTOR_STATION` to a **new absolute path** outside the checkout. In the commands below, `studio` is the absolute executable of your installed native Studio (or its packaged `Personas` entry point); `tools` is an installed toolchain directory with a `bin` subdirectory. `bench` and `receiving` are the facilitator's **trusted host launchers** for local and receiving work. Provision them for the new station; do not reuse a policy that points at another project's files. The desk launcher requires both rather than silently dropping a missing policy. These are host settings, not tools or a procedure assigned to a persona.

```sh
# Run in the runtime checkout, with the variables above set to the deployment paths you verified.
set -eu
umask 077
s=${OWNER_CONTRIBUTOR_STATION:?set a new private station path}
mkdir "$s"  # must be new; do not overwrite a previous observation
mkdir -p "$s/Bundle" "$s/control" "$s/shop/project" "$s/shop/home" "$s/shop/scratch" \
         "$s/observation" "$s/native-exports"
for desk in owner contributor; do mkdir -p "$s/desks/$desk/config" "$s/desks/$desk/state"; done
ln -s "${studio:?absolute native Studio executable}" "$s/Bundle/Personas"
ln -s "${tools:?absolute toolchain directory}" "$s/Toolchain"
install -m 700 "${bench:?prepared local launcher}" "$s/control/bench"
install -m 700 "${receiving:?prepared receiving launcher}" "$s/control/receiving"
cp examples/owner-contributor/observation.tsv "$s/observation/attempts.tsv"
```

## Set future reasoning, not historical results

For the recorded baseline, restore the **private SQLite closing archive** identified in the [BC-17 record](../solar-bond-strip/decisions.json), SHA-256 `a5204a9c77d4af6dedfa3114e156b8f5520f7fcc6789c3b953b9b049a17fc8eb`. The public JSON is **not** the archive. Without that archive use your own authorized settled baseline and record it as a different station, with its actual residents. Do not try to reconstruct a persona's history from a published summary.

```sh
ai-personas neural --state-root "$s/Studio" restore "$archive" \
  --sha256 "$archive_sha256" --working-directory "$s/shop/project"
```

Verify the workspace has no listener, eligible earlier activity, unsettled call/effect or pending outside request. `neural runner-status` reports the listener; it does not by itself check those other conditions. The restored BC-17 source has three stopped participations. Its existing Codex OAuth binding must be authorized on this host. If it is not, use Studio's [**Workspace settings → Model connection → Codex login**](../../docs/desktop.md) to select your existing authorized login. Do not copy a token or assume a Platform key is the same connection.

For this Astra station select the following on the **copy**, not the source. Studio's **Change reasoning settings…** also edits a resident's selection on its existing connection. No need to initialize, invite or resume anyone during setup.

```sh
ai-personas neural --state-root "$s/Studio" --operator Facilitator configure mara \
  'Future calls on the participant station' --model gpt-6.2-astra --effort high
ai-personas neural --state-root "$s/Studio" --operator Facilitator configure oren \
  'Future calls on the participant station' --model gpt-6.2-astra --effort xhigh
```

## At the session

1. On the actual desktop, use the **same checkout and station** for the three commands in the [facilitator's protocol](README.md#set-the-desks). `arm` stays in the foreground. Compare the displayed resident IDs, modes, models and efforts with the intended station. For the Astra setup expect `chatgpt_oauth` for both. Stop if they differ. The private `Capture/armed.json` and `armed.sqlite` refer to the same pre-admission read; neither is a new provider receipt.
2. Leave that terminal open. A desk checks for the live observer of its own `Studio/neural.sqlite` and `Capture` before opening. A PID or a copied `armed.json` is not enough. This is a local readiness check at window launch, not authentication of a human or a guarantee of uninterrupted observation. Do not move or replace the SQLite file during observation. The observer refuses to finish a capture across a replacement or a journal that moved backwards. If observation is lost during the session, record the gap. Do not restart the commission to get a better trace.
3. After the participants are done, finish admission, let admitted work settle, then `touch "$s/finish-observation"` and wait for the observer to exit. Keep `Capture/observation.json` and the private archives. If a capture or stop file already exists, preserve the station instead of erasing it for a retry. Use a **separate copy** for any facilitator-only window checks.
