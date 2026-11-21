# PC power supply: a Studio trial

On November 21, 2026, a requester entered a PC SMPS brief in native Linux Studio. Two newly created personas, Nia and Ellis, received invitations. Both used the direct OpenAI Platform API-key connection with `gpt-6.1-sol` at medium reasoning effort. Neither had an assigned profession. The model authored a different numerical character for each.

**Conventional ATX result:** the [continued trial](#six-layer-atx-candidate-and-final-disposition) produced an editable, routed 45 W candidate with reproducible manufacturing plots. The personas did not issue it for fabrication, and independent checks did not establish connector-voltage compliance or verify every orderable component.

## The first delivery: PC60 revision A

The requester asked for an editable circuit and files a manufacturer could use without the requester editing them. Input, output and PC assumptions were left to the personas. Ellis chose an external AC/DC adapter and a 24 V to 12 V switching module for a small desktop with a native 12 V terminal input. Nia compared the wiring and schematic, wrote acceptance documents, corrected a measurement-fixture instruction and inspected the frozen archive. This is a wired module assembly; it includes neither a custom switching PCB nor the rails and control signals of an ATX motherboard supply.

Ellis issued `PC60-revA-manufacturer.zip` as **partial** against the finished-supply request. Its supported use is a guarded engineering prototype targeting 12 V, 5 A (60 W). An actual PC interface, fault overvoltage protection, enclosure, bonding, thermal performance, EMC and system operation remain unresolved. No purchase, hardware build or measurement took place. Both personas chose to stop.

| Check | Observation |
|---|---|
| Autonomous work | One brief, 40 peer messages and no subsequent operator task messages, amendments, reviews or resumptions. |
| Direct inference | 114 accepted decisions (53 Ellis, 61 Nia) and 12 rejected responses. All 126 completed provider outputs matched the retained responses byte for byte. Reconstructed inputs matched the prompt and image receipts. All 110 accepted calls after initialization matched their owner’s character use. |
| Time and usage | 12 min 27 s from the first admitted call to the last completed one. The provider reported 1,914,330 tokens, including 837,000 cached inputs. |
| Execution | All 65 local project command records matched their originating outbox operations. 58 returned exit zero, five had nonzero exits and two had no exit code. |
| Manufacturing archive | 15 members passed ZIP CRC checks. All 14 entries in `SHA256SUMS.txt` matched the original files. The editable SVG parsed, the schematic PDF opened as one page, and all five JSON connections agreed with the wiring CSV. The BOM had eight entries. The four documents Nia contributed matched her exact retained bytes. |
| Qualification | All 22 qualification rows were marked `NOT_RUN`. The unit test record contained no measurements. The included module datasheets describe the parts, not a qualification test of this assembly. |

In Studio the operator saved both the author’s ZIP unchanged and a single **Shareable ZIP** with `Delivery.html` and `Files.zip`. The latter retained the former byte for byte along with all five remaining items. The manufacturer ZIP’s SHA-256 is `02c1515dba6bceded5717303c68f393283b94092f10e9e75f0b1d4328e891a54`. The verified delivery is `delivery:delivery-215e4bf90bc1eb2c7f93af12ff95936b@1`.

![The schematic inside Ellis’s actual ZIP, beside the supported use and remaining work.](assets/studio-pc60-schematic.png)

The delivery viewer opened `circuit.svg` directly from the issued ZIP. Returning to its directory preserved the `circuit.svg` search. Selecting the converter datasheet opened its nine-page PDF; moving to page two displayed the specification table. The journal also held Nia’s archive-inspection decision in place while later calls arrived, and opened her epoch-one character inputs for that exact decision.

## A runtime finding

The first run used [the autonomous handoff build](https://github.com/ai-personas/ai-personas-runtime/commit/a402ea71e11891c4ed5d046943f0ff00070fb0ae). All twelve rejections omitted the next appraisal choice. The host correctly rejected the responses before dispatch, but its advertised schema described that field as optional even for an active character that did not choose `persona.evaluate`.

[The corrected schema](https://github.com/ai-personas/ai-personas-runtime/commit/10d3b8b) distinguishes initialization from active work and preserves the explicit evaluation-operation alternative. Replaying the original active responses through the new advertised schema admitted all 110 that had passed and identified all 12 omissions. In a separate fresh Studio activity a persona explained `NOT_RUN` in one sentence; its two initialization and two active calls used the revised interface and were accepted. That small run is not a general estimate of rejection rates.

## Second brief: conventional ATX and actual switching circuitry

The same two personas received a **new activity** for a 24-pin ATX motherboard and four-pin CPU supply, explicitly requiring an editable switching circuit and files for prototype manufacture. The prior PC60 activity remained stopped. No task instructions were added after the new invitation.

At a runner-update checkpoint, the participants had made 270 accepted decisions, exchanged 87 peer messages and executed 250 local commands. All 271 completed provider inputs and final outputs were checked, including one rejected response. They had obtained component references and negotiated responsibilities but had not issued a new circuit or fabrication package. Their discussions repeatedly referred to a generator they intended to write as though it should already exist. Calling the earlier 12 V assembly an ATX result would be incorrect.

During this run both personas chose the new complete-text option for local commands. Across 62 accepted decisions, 70 chosen stdout/stderr bodies matched all 222,877 original UTF-8 bytes; 58 exceeded the default excerpt. This demonstrates delivery of chosen observations. It does not demonstrate that receiving more output solved the design task. The runner was drained for software updates with unfinished responsibilities intact; this checkpoint is not a persona decision to stop.

### Where the initial ATX run stopped

The updated Studio continued the same saved activity. The personas authored three component tables (27 switching-stage components, 19 control/negative-rail components and three connectors), two generators, three harness footprints, wiring notes, a 49-row procurement list and an assembly/test plan. The 24 motherboard, four CPU and two input pin identifiers matched the respective footprint pad identifiers. This check says nothing about pad geometry, current rating or electrical operation.

**There was no ATX delivery.** Neither an integrated KiCad PCB nor an integrated KiCad schematic was present in the project. No fabrication set or hardware qualification was demonstrated. The participants raised, but did not resolve, a possible package mismatch: their procurement file still paired `SS54-E3/57T` with `D_SMA`. A local download named `ss54_verified.pdf` actually began with an HTML doctype. A file named as a datasheet did not verify that part.

The evaluator finished the paid run with both personas still choosing `ready`; no task amendment, corrective message, or operator-authored design file was supplied. The local operator saved 11 unchanged working files separately from the issued PC60 ZIP. They are **work in progress, not files for a manufacturer**. The workspace retains both activities and their individual continuation states.

| Final ATX measurement | Observed |
|---|---:|
| Accepted decisions / rejected responses | 532 / 3 |
| Verified completed provider inputs / terminal outputs | 535 / 535 |
| Verified ordinary character uses | 532 |
| Persona-to-persona messages / operator follow-up task messages | 147 / 0 |
| Local command receipts | 512 |
| Time between first admission and last completed call, including runner handovers | 39 min 27 s |
| Reported tokens, including rejected calls | 7,375,969 (3,650,294 cached input) |

**Conclusion:** the trial demonstrated independent research, cooperation, digital prototype delivery for the narrower PC60 brief, and enough evidence to see where work broke down. It did **not** demonstrate autonomous delivery of a manufacturer-ready ATX supply. Repeated tool use and successful model calls are not substitutes for integrating and verifying the required hardware design.

## ATX continuation: working in the shared project

In the original ATX run, 508 of 512 local commands named the shared project by absolute path while starting in a disposable directory. None captured a file in `outputs`. Of 532 accepted decisions, 531 selected no exact resources for the following call. The notes often identified the right files; the next operation nevertheless listed or printed them again.

The [shared project change](https://github.com/ai-personas/ai-personas-runtime/commit/9bfb5a8) lets a persona run a program directly in the configured project, capture named files and keep chosen text editions open across decisions. The desktop identifies these runs in the timeline. In a disconnected dispatcher check, captured text and byte ranges remained exact after editing one host file and deleting another. A failed replacement retained the old selection, a peer received none of it, and a provider-capacity recovery supplied pointers instead of bodies.

The evaluator clicked **Run workspace** in a freshly built Studio. Nia and Ellis continued the same ATX activity, under the same numerical characters and `gpt-6.1-sol` settings. No new invitation, circuit instruction, board file or diagnosis was given to them. The workstation supplied stock KiCad 9.0.9 symbol and footprint libraries, its Python bindings and Freerouting 2.5.0. A missing KiCad runtime library was installed. After the native router's worker threw a serialization exception and the process failed to exit, the evaluator terminated that failed process and installed the upstream JVM edition as the routing executable. The participants made their own placement, routing and design choices. This was a **workstation-assisted** trial; it is not an isolated measurement of one software change.

An intermediate pair of native KiCad files had 50 schematic components, 21 nets and 147 reference/pin assignments. On copies, all 147 matched the PCB after normalizing schematic sheet prefixes. Both connector pin maps matched the intended ATX and CPU supply interfaces. One routed version contained 488 track segments and 54 vias, with zero unconnected items. It still had 13 courtyard-overlap errors, one electrical-rules error, 152 reported schematic parity differences and five inductor footprint-name differences in the procurement list. The participants subsequently revised the geometry, the negative-rail circuitry and the power target. These measurements apply only to the recorded intermediate edition, not to a later fabrication package.

![Nia’s Studio decision while inspecting the routed power supply: zero unconnected items did not establish acceptable switching-current paths.](assets/studio-atx-routing-review.jpg)

### Six-layer ATX candidate and final disposition

The personas created a 24 V DC input, 45 W combined-output design for a low-power PC, with four main/standby switching stages, a negative rail and a supervisor. The +12 V target is 2.5 A shared between the motherboard and CPU connectors. They corrected unsupported inductor choices, revised the negative-rail circuit, fixed placement and routing defects, and added internal distribution planes. Nia’s exporter froze the board, schematic, libraries, procurement and assembly files, Gerbers, drill files, qualification forms and checks together. The candidate itself still says **“NOT AUTHORIZATION TO FABRICATE.”**

| Independent check on the saved six-layer candidate | Result |
|---|---|
| Archive integrity | 78 files including `MANIFEST.json`; all 77 manifest digests and all ZIP CRCs matched. Board SHA-256 `3f1d7c91ebac0efc634fa7e72450f3bc4c8aed9c0970f21cb97439c8f8b3d682`. |
| Editable circuit and PCB | KiCad 9.0.9 extracted 57 components, 25 nets and 164 pin-to-net assignments. All assignments matched the PCB after normalizing sheet prefixes. Both connector maps matched. The PCB had 436 track segments, 89 vias, seven zones and four mechanical mounting holes. |
| BOM and placement identities | All 57 electrical references, values and footprint IDs agreed between the BOM and editable source; BOM quantities and manufacturer numbers matched `design.json`. This checks **consistency**, not part availability. |
| Board and fabrication geometry | Board-only DRC found zero violations and zero unconnected items. Independently re-exported all 13 Gerbers (six copper, two masks, two pastes, two silkscreens and outline) and the two Excellon files. Gerbonara parsed all 15 and matched their ordered geometry to the supplied files, including 143 plated and four non-plated hits. |
| Full electrical and parity checks | ERC found one dangling label on unused ATX pin 20 and 57 off-grid warnings. The schematic-parity run reported 169 net conflicts and four extra mechanical footprints; it did not pass. No DRC exclusions were configured. |
| Component and electrical suitability | D1–D4 and D90 changed from SMA to SMC and now agree across PCB and BOM. The BOM still says `SS54-E3/57T`; [Vishay’s SMC family document](https://www.vishay.com/docs/88885/ssc53l.pdf) names **SSC54**. The exact BOM number was not verified. The authors’ own voltage analysis finds the fixed regulators can consume the entire full-temperature ±5% allowance before wiring loss. All 14 qualification-record rows had empty measurement and pass/fail fields. |

The evaluator saved the **unchanged candidate**, as `ATX45-six-layer-3f1d7c91-candidate.zip` (SHA-256 `aa7788c2fc6043b00431f5480bb26382cfc9854ac0e8c671bc8b5c881beaaa0c`). This ZIP was assembled from the participant-frozen source tree; it is **not** a Studio-issued delivery. At close, Studio showed zero ATX packages, 22 activity-local library assets, and both participants still choosing `ready`. Ellis was reconsidering the negative-stage switching paths. Hardware was not ordered or tested.

![The saved ATX activity: both participants have continuing intentions and the delivery count is zero.](assets/studio-atx-final-disposition.jpg)

**What happened during the retry.** There was one original ATX brief and no subsequent requester messages or amendments. The evaluator selected high effort for part of the run and later returned Ellis to medium, keeping both on `gpt-6.1-sol`. Across 2 h 13 min, the retry had 1,115 accepted decisions, four rejected complete responses, 269 peer messages and three failed inference attempts. All 1,119 completed inputs and terminal outputs matched their retained receipts, as did 1,115 active-character uses. Of 1,118 verified command receipts, 1,117 used the shared directory; 54 commands captured 138 named file observations. Neither persona selected `workspace.open` in this live trial. It was verified separately and cannot be credited with producing the board.

The provider reported **21,427,471 total tokens**, including 7,858,434 cached input tokens, across the 1,121 attempts with usage. One HTTP 520 had no reported usage. Two output-capacity failures each reported 128,000 output tokens; neither produced an accepted decision or tool operation. The evaluator released the old execution pauses. On the updated build Ellis received the failed-attempt pointer with its ordinary inputs and made an accepted decision. Nia’s later exhaustion scheduled fresh inference, but the evaluator’s **Finish run** prevented a new admission. At close no model or tool calls remained in flight.

**Observation boundary:** the evaluator provided no component substitution, circuit edit, routing fix, design coaching or manufacturing authorization. On the shared host, during a broad search Ellis listed evaluator filenames and inspected footprint/track counts on copies of its own earlier PCB. The copies and verification programs were moved out of the study directory. This was not a blinded experiment. The [gateway](https://github.com/ai-personas/ai-personas-runtime/commit/7e80eb8) and [output-capacity](https://github.com/ai-personas/ai-personas-runtime/commit/7429571) recovery changes retain the original failures; they do not turn them into successful calls.
