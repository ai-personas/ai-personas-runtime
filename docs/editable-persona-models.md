# Numerical workspaces

Open the desktop with `./personas --gui`. Choose **New workspace**, give each persona a name and a starting character profile, select a working directory and connect the reasoning provider. Creating the workspace performs no inference. Create a task, choose its invitees and start the runner to see their decisions.

The desktop and terminal use the same SQLite workspace. The following commands create one from a file of starting profiles and an existing direct provider configuration.

```sh
./personas --neural --state-root ./society init --seeds profiles.json \
  --working-directory ./work --provider-config provider.json
./personas --neural --state-root ./society request 'Plan our community garden' --invite mira,orin
./personas --neural --state-root ./society run
./personas --gui --progress ./society
```

You can also select **OpenAI API key** and choose a private key file. The terminal equivalent is `--platform-key-file FILE --model MODEL --effort EFFORT` in place of `--provider-config`. The file can contain the key as plain text or be an existing Studio provider credential file. The workspace stores its location and a credential fingerprint. The provider reads the secret at inference time.

The working directory must exist. The provider file contains configuration and credential references. Set the referenced environment variables in the process that runs inference. The profiles file has this form.

```json
{"personas":[
  {"id":"mira","label":"Mira","seed":"Curious and improvisational, warm to strangers, initially calm."},
  {"id":"orin","label":"Orin","seed":"Quiet and conscientious, initially alert to ambiguity."}
]}
```

The first calls translate the attributed seeds into candidates. Subsequent calls observe and adopt exact packages before the personas see the task. Each active model evaluates eighteen value/known input pairs into twelve inclinations. The provider can propose literal coordinate edits, compare a candidate in matched situations and decide whether to adopt it. Rollback retains the intervening history.

In **Character Lab → What if…**, mark a situational input as known, adjust its value and observe the twelve forward outputs. The chart compares the previous package at the same inputs when there is a predecessor. For an initial model the comparison is its all-unknown baseline. This operator preview makes no model call and writes no experience. **Development** links accepted decisions to their supplied character, recorded questions and observations.

A persona can search experiences across its own participations with `experience.search`, select exact records, and retain a question with `development.update`. The question can later motivate a candidate and a follow-up after adoption. Actual input receipts name the completed provider request, owner, package, numerical fingerprint and appraisal used.

Use `ai-personas character` to see the feature contract. `ai-personas character MODEL.json --compare OTHER.json --appraisal SITUATION.json` distinguishes the complete package, canonical numerical values and actual output differences in that situation. The all-unknown baseline can stay unchanged after a conditional edit.

Personas can exchange messages, invite peers, originate work, publish and discover shared artifacts, import files from the selected working directory, evaluate WebAssembly assets through the compute.wasm.v1 interface, and deliver exact file revisions. Run, pause, amend a request, give feedback, inspect the evidence or export delivered files from the studio or the numerical CLI. Earlier journals retain their own execution interfaces.

The numerical representation and the studio build on the implementation referenced by the November design. [Design and conformance criteria](https://github.com/ai-personas/ai-personas-design/tree/7d86efb31a0323e03166a5c5212cdcd8b2e32f36) · [Source baseline](https://github.com/ai-personas/ai-personas-runtime/tree/1aabb15c56b8a1f3c0403bdb28feeedb57c5bd7e)
