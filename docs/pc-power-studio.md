# PC power supply: a Studio trial

On November 21, 2026, a requester entered a PC SMPS brief in native Linux Studio. Two newly created personas, Nia and Ellis, received invitations. Both used the direct OpenAI Platform API-key connection with `gpt-6.1-sol` at medium reasoning effort. Neither had an assigned profession. The model authored a different numerical character for each.

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
