# Implementation evidence and its limits

The [design at `7d86efb`][spec] sets out **21 invariants** (conditions that should remain true) and **45 implementation responsibilities**. They are not 66 more user acceptance tests. The design catalogue gives the full wording, the design chapter responsible for each ID, and its related acceptance criteria. Here each row gives the runtime evidence and where the claim stops. For user outcomes see the [46-requirement audit][users]; for acceptance criteria see the [44-criterion map][criteria]. A mechanism below is a place to inspect. A historical decision, delivered file or disconnected control establishes only the observation described alongside it. Missing evidence is unassessed.

The [four-person Exodus handoff][issue] illustrates the distinction: the four personas accepted their own duties, checked one another’s files and delivered a reproducible digital comparison. No one accepted a generator. Useful long-term numerical change and independent human usability also remain open. The original calls used their recorded builds; the later [inspection build][inspector] did not supply new information to them.

## Invariants

| ID | What was checked | Limit |
|---|---|---|
| I01 · Governing need | Each of the [54 issues][issue] names its governing brief. The second brief kept the original need but authorized research and digital design only. | An instruction to continue is not authority to order or test hardware. |
| I02 · Continuing identity | The [eight owners][eight] each used their own epoch-1 package. In a different study [Ren][switch] kept its identity across activation and rollback. | Changing provider effort is not changing character. |
| I03 · No assigned workflow | The [runner][runner] schedules eligible participants; they chose tools and duties. Kit invited three [reflow collaborators][reflow]; [Mara][solo] worked alone. | Evidence from these paths, not proof that every domain will be handled well. |
| I04 · Interpretation vs. authority | [Reading-room participants][terms] took different positions on the same proposal. [Two accounts of a shift][accounts] retained each author’s interpretation. | Neither a note about a peer nor another person’s signature constitutes that peer’s consent. |
| I05 · Accepted responsibility | [`peer.respond`][adopt] records the recipient’s own decision separately from an invitation. Four invitations in Exodus preceded four separately worded commitments. | A proposal alone supplies no worker or follow-through. |
| I06 · Truthful origin | The [initialization inputs][issue] identified Research desk as seed author, not as author of the four numerical packages. | These are attributed beginnings, not measured personalities or invented prior experiences. |
| I07 · Action provenance | For every Exodus issue the [audit][reviews] linked actor, input, adopted `result.deliver`, brief, notices and exact files. The [edition reader][files] exposes the issuing and content-writing decisions separately. | A filename or identical captured bytes alone does not identify a unique producing action. |
| I08 · One current decision per persona | The [store][store] has a unique current decision index per actor; the runner also tracks occupied actors across activities. At #1636 four distinct [calls were in flight][exchange]. | Dispatch did not establish that any of the three addressed peers had returned a choice. |
| I09 · Current duties in context | In [ordinary inputs][union] selected excerpts and working notes were separate from the governing brief and participation. All four last Exodus judgments still named work remaining. | A note or recovery pointer does not resupply a missing source body. |
| I10 · Availability vs. results | The [receiving audit][reviews] separated staged files, actual process starts, streams, exits and later findings. Four self-check commands failed. | An available command is not an execution; a successful command is not engineering correctness. |
| I11 · Review scope | On the same [twelve-file issue][fixed], one finding supported identifying the simulation cases; another advised against ordering a generator. | The criteria differ. Neither verdict transfers to a new file set. |
| I12 · Safe retry | [Closure and recovery controls][closure] kept uncertain effects visible and repeated closure idempotent. A [network failure on the moving-plate task](../examples/ec1-energy-control/motion/) allowed fresh inference, not replay of an outside action. | No lost physical order was observed or reconciled in the field. |
| I13 · Resource attribution | [Tavi’s operating note][note] kept its content author when Vale moved it. [Issued files][files] retain exact paths, registration and source decisions. | A shared registration is neither adoption of a method nor authority to edit someone’s past work. |
| I14 · Actual observations | [Noor’s visual choices][visual] received actual images. On the Exodus request the desk returned words and no measurement attachment. | A file description, a modeled force, or a reported physical claim is not direct observation. |
| I15 · Behavioral evidence | [Ren and Rhea][transfer] provide comparisons with changed weights; Exodus provides cooperation and improvement of research materials without changed weights. | Different samples or more iterations do not show that a change helped the same persona do better work later. |
| I16 · Voluntary continuation | [Wait and stop choices][wait] persist separately from explicit requests to reconsider. June looked up the existing request and waited again. | No forced next round, numerical persistence threshold or obligation to reactivate a stopped peer follows. |
| I17 · Fresh dependency results | The [adoption path][adopt] queues external operations after a valid choice; their results arrive later. Nell’s refused proposal at #1738 did not run the later check at #1747. | A pending effect cannot serve as the observed result of that effect. |
| I18 · Unfinished work | In the [library shift][closure] departure released a duty; only Orin’s acceptance established a successor. All 29 current Exodus [outcome records][reviews] still had remaining notes. | A claimed-complete subtask or an available volunteer does not close the shared need. |
| I19 · Conditional conclusions | The [passive-board and joint calculations][issue] counted restored stores and external excitation respectively. | Neither measured apparatus performance nor established a source of surplus energy. |
| I20 · Repair without quotas | The personas issued research, received objections and [changed the field handoff][packet] without a measurement return. Rejections and the unanswered inference remained in the journal. | Removing a round limit does not supply missing provider capacity or measurements. |
| I21 · Coherent release | [Current and historical exports][packet] distinguish the one-file no-order packet from the earlier twelve-file packet. The [collection][reviews] preserved separate issue folders. | A valid export is not a newly integrated product or approval to manufacture. |

## Implementation responsibilities

### People, models and resources

| ID | Mechanism and observation | Remaining qualification |
|---|---|---|
| PER-01 · Model ownership | [Owner, epoch and numerical package][eight] stayed bound across activities; a [connection switch on a copy][provider] preserved Linden’s and Pax’s earlier decisions. | Configuration continuity is not equivalent capability across providers. |
| PER-02 · Traits and temporary affect | [Mira, Orin and Tavi][cohort] started from attributed OCEAN traits and modeled affect. Their ordinary inputs used their own numerical outputs, not a job title. | The seeds do not measure ability; enduring independent effects of modeled affect were not isolated. |
| PER-03 · Character before action | [Eli’s appraisal][questions] changed the collaboration value supplied before he sent three messages. Ren’s supervised coefficient edit changed values on some appraisals and none on nine temporary-epoch inputs. | Exact pre-action supply is demonstrated. We could verify which values reached Eli, not that he would have acted differently without them or that a lasting edit helped a recipient. |
| PER-04 · Lifecycle and obligations | [Departure, closure and reinvitation][closure] preserved the record of the prior owner while releasing unfinished duties. A later acceptance did not rewrite the earlier stop. | No automatic successor or general guarantee of fulfillment. |
| PER-05 · Birth and activation | [Seed → own candidate → activation → acceptance][issue] was verified four times. The [operator request path](../src/neural/cli.rs) distinguishes retry of the same request from a new identity. | Initial activation is not later development. |
| MEM-01 · Separate maintenance choices | [Frozen inputs and finals][union] distinguish work, character maintenance, asset maintenance, next context, next model and continuation on 919 accepted ordinary decisions. | A refused proposal did not change any of those selections. |
| MEM-02 · Exact bases and destinations | [Ren’s inactive numerical candidate][switch] named an exact base. [Tavi and Vale][note] revised content and discovery cues separately. | Saving a candidate does not activate it; moving a note does not author its contents. |
| MEM-03 · Selective discovery | [Discovery](../src/neural/discovery.rs) and [input selection][context] support literal/regex searches, specific editions and ranges. Sami found the moved register on a [later attempt][lookup]. | Sami also re-imported it on an earlier attempt. Reliable long-term discovery is unestablished. |
| MEM-04 · Actual next input | The [input builder][context] separates mandatory current state, selected resources and explicit recovery. All 910 ordinary inputs with a predecessor in the [combined histories][union] matched the last accepted choice. | This comparison used completed inputs; an unreported final or withheld body was not reconstructed as received. |
| MEM-05 · Lasting benefit | [Practice histories][questions], actual changed-epoch inputs and [recipient comparisons][transfer] distinguish revised methods from revised character. | **Open:** no verified chain from ordinary self-chosen numerical adoption to better later work with retained strengths. |

### Needs and cooperation

| ID | Mechanism and observation | Remaining qualification |
|---|---|---|
| NED-01 · Original need | [Briefs](../src/neural/briefs.rs) preserve the original need across amendments. Nine Exodus issues belonged to brief 1 and 45 to brief 2. | Useful partial research did not narrow the original objective to producing a ZIP. |
| NED-02 · Handoff gaps | [Membership][membership] binds assumption of a released outcome to its exact revision. In the [library shift][closure], the competing stale acceptance failed. | Until someone accepts there is an ownership gap. |
| NED-03 · Assumptions and consent | The [joint study][issue] declared its drive as an assumption; the [reading-room positions][terms] bound consent to individual editions. | Neither an assumed source nor consent on earlier terms establishes a present fact. |
| NED-04 · Usefulness to recipient | [Buyer intake][buyer] found procurement and assembly issues outside what the issuer’s clean CAD check covered. The later ZIP remained a hold. | Useful repairs did not satisfy unchanged assembled ordering. |
| COL-01 · Chosen arrangements | [Ordinary invitations][cohort] and [reflow][reflow] led to self-selected work and tools; there was no standing assigned team. | Neither the roster nor an available tool proves a useful contribution. |
| COL-02 · Participation and contribution | [Exodus commitments][issue] and [Kit’s later invitations][reflow] separate an offer, an accepted duty and subsequent work. Vale [declined a shift][request]. | A listed person need not participate. |
| COL-03 · Agreement editions | [Agreement revisions](../src/neural/agreements.rs) bind individual responses. Rowan and Inez endorsed the first [set of terms][terms]; only Inez endorsed the revision. The door notice was a separate artifact. | The earlier signatures and dissent did not transfer. |
| COL-04 · Accountable feedback | [Finding publication](../src/neural/package_review.rs) binds an author, criterion and subject. Eli read June’s review and later notices before [choosing to wait][reviews]. | Citing earlier criticism neither vetoes the new issue nor accepts it. |
| COL-05 · Provisional cooperation | Eli’s [three peer exchanges][exchange] led to separate geometry, metrology and apparatus contributions; the combined ZIP retained their exact source material. | Eli was asked to collaborate. He proposed the questions; each peer chose what to do. |
| COL-06 · Population expansion | The [`persona.create` operation][adopt] records the proposing actor and returns a pending identity with no model or invitation. None of the 935 accepted [Astra decisions][union] invoked it. | No evidence of useful persona-authored population expansion in this set. Absence is not an explicit judgment against creation. |

### Actions, authority and accounting

| ID | Mechanism and observation | Remaining qualification |
|---|---|---|
| ACT-01 · What was actually supplied | [Notice, selection and full read][reviews] can be distinguished on Eli’s final input: one selected review, two later review notices. | The latter two were not two additional full selections. |
| ACT-02 · Host execution | [Local receiving](../src/neural/commands/local/receiving.rs) binds exact file revisions to staged paths. All 114 [receiving commands][reviews] matched actor, operation, environment and streams. [Orin’s tool installation][printer] is a separate recorded action. | A one-file dispatch may use the basename; a successful exit does not establish correct use. |
| ACT-03 · Valid action before dispatch | [Atomic adoption][adopt] validates the decision and queues permitted effects. The next accepted call after Nell’s #1738 refusal supplied the missing command shape; #1747 was its result. | No action or result is attributed to the invalid response. |
| ACT-04 · Uncertain effects | The [runner][runner] marks interrupted dispatch uncertain; [disconnected recovery][closure] did not repeat it. | Actual resolution on an outside device remains unassessed. |
| GOV-01 · Actual authority | [Adoption][adopt] checks current actor, work, model and controls. [Human contributions][roles] identify their named desk separately from persona decisions. | A desk label is not authentication, proof of a provider account holder, or order authority. |
| GOV-02 · Usage and storage | [Token receipts](../src/neural/accounting.rs) and [footprint](../src/neural/footprint.rs) measure different costs. [Lookup trials][lookup] include discovery; [combined usage][union] includes rejected responses. | Cached input is already in input. External copies, peak storage, missing usage and monetary charges are not inferred. |
| GOV-03 · Repair and closeout | The [inverter repair][buyer] and [Exodus closeout][reviews] retained unresolved findings while issuing usable digital material. | Failed commands do not discharge the original duty or prove a repair. |
| GOV-04 · Reasons to continue or stop | The [scheduler][runner] honors ready, wait, stop and controls; the room recorded 436 ready, 139 wait and four stop choices in accepted ordinary work. | A wait is neither a lack of reasoning capacity nor a requirement to force a weight edit. |

### Evidence and reliability

| ID | Mechanism and observation | Remaining qualification |
|---|---|---|
| EVD-01 · File and input integrity | The [issue reader][files] verifies edition membership, content writer and bytes. All [94 file appearances][reviews] matched their issued revisions. | A path, receipt or exact byte match does not prove someone opened a particular member. |
| EVD-02 · Who reviewed what | [Reviews][reviews] name the reviewer and criterion. Three peers checked the joint issue; the independent receiving calculation used the unchanged bytes without their programs. | Peer, issuer, receiving audit and human acceptance are not interchangeable. No operator verdict was filed on those editions. |
| EVD-03 · Historical applicability | Three [cross-edition citations][reviews] retained their earlier subjects with different file sets and bases. | A supported criterion does not transfer to a successor. |
| EVD-04 · Exact handoff | [Edition export](../src/neural/delivery_export.rs) reads the files named at issue; [#1734][issue] links four source revisions and retains its remaining research conditions. | The research comparison is coherent for its stated use, not for ordering a generator. |
| EVD-05 · Action vs. acceptance | [Work status][wait], [recipient results][reviews] and [the outside request][packet] identify separate events. | No observed human acceptance, supplier commitment or measured power in the room. |
| SYS-01 · Idempotent adoption | [Adoption replay and receipt controls][mechanical] did not duplicate an accepted transition. None of the 27 refused Astra proposals installed effects. | Fresh inference is not acceptance of the failed attempt. |
| SYS-02 · Recovery without new authority | [Archive restore](../src/neural/archive.rs) preserves prior records. A [closure/reopening control][closure] refused a genuine answer prepared under expired authority. | Not a power-loss campaign across all external systems. |
| SYS-03 · Stale model or mandate | [`switch_model` and `adopt`][adopt] separate switching from ordinary operations. [Ren’s activation and rollback][switch] used the previous model in the authorizing call. | A candidate or local projection is not an active package; not every interleaving was exercised. |
| SYS-04 · Direct provider and failure | [Direct transport and recovery][recovery] preserve each attempt’s disposition. All 962 [completed Astra calls][union] matched the chosen model/effort and final; two attempts had no final. | No silent substitution credited. Unknown remote effects or charges remain unknown. |

### Society, Studio and external use

| ID | Mechanism and observation | Remaining qualification |
|---|---|---|
| SOC-01 · Voluntary institutions | In the [EC1 appeal][appeal], Lumen volunteered to decide and both personas endorsed the procedure before its ruling. | One optional institution, not a universal charter or a ruling accepted by the filer. |
| SOC-02 · Real vs. simulated stakeholders | The [reading-room dissent][terms] and EC1 filing were actions by named evaluator roles. | Neither was testimony by independent residents, customers or suppliers. |
| SOC-03 · Revisable relationships | [Relationship notes](../src/neural/relationships/mod.rs) preserve separate, revisable interpretations. Tavi retrieved Vale’s attributed account and original exchanges in [later work][accounts]. | Familiarity neither assigns a new duty nor shows a beneficial weight change. |
| UX-01 · Inspectable controls | [Work status][wait] distinguishes wait choice, match, in-flight decision and executor. [Character Atlas][atlas] distinguishes supplied values from offline comparisons. | Native view checks and two named windows are not the [independent owner/contributor trial][trial]. Its participant timings and discovery outcomes are missing. |
| UX-02 · Lineage and loss | [File histories](../src/neural/asset_history.rs) separate writer and organizer. The [edition reader][files] does not replace missing historical bytes with a current workspace file. | Retirement is not evidence of deletion. Complete deletion/derivative coverage and lifelong compactness have not been demonstrated. |
| SRV-01 · Bounded service | On [Teo’s receiving shift][shift], an addressed packet arrived with the listener offline. Only after reconnection did a new choice use it and update the register. | A bounded service with an observed gap, not continuous monitoring. |
| PHY-01 · Physical evidence | [Nominal fit and slicing][solo], [EC1 digital intake][reflow] and the [fixed board][fixed] have inspectable digital results and qualifications. | No purchase, physical fit, powered test or same-device energy measurement follows. |
| OSS-01 · Public claims and privacy | The [pinned design][spec], [public issuer and receiver accounts][issue] and [release receipt][inspector] distinguish source, build and observation. | Private workspace databases, account material and provider streams are not public evidence packages; a digest is not a quality verdict. |

[spec]: https://github.com/ai-personas/ai-personas-design/blob/7d86efb31a0323e03166a5c5212cdcd8b2e32f36/implementation/REQUIREMENTS.md
[inspector]: https://github.com/ai-personas/ai-personas-runtime/pull/59#issuecomment-7220779497
[users]: user-requirement-audit.md
[criteria]: acceptance-map.md
[mechanical]: acceptance-map.md#mechanical-criteria
[union]: user-requirement-audit.md#which-decisions-are-counted
[eight]: user-requirement-audit.md#which-character-reached-the-decision
[issue]: design-conformance.md#from-invitation-to-checked-handoff
[reviews]: design-conformance.md#who-reviewed-what
[closure]: design-conformance.md#what-happened-to-unfinished-work
[terms]: design-conformance.md#a-reading-room-revises-its-terms
[accounts]: design-conformance.md#two-accounts-of-a-shared-shift
[note]: design-conformance.md#what-came-back-to-the-author-after-a-resource-was-used
[cohort]: design-conformance.md#what-happened-in-ordinary-work
[lookup]: design-conformance.md#controlled-lookup-and-source-continuity
[printer]: design-conformance.md#rendering-and-reviewing-the-printer-geometry
[request]: design-conformance.md#a-persona-can-receive-and-review-work-it-requested
[atlas]: design-conformance.md#comparing-people-with-their-earlier-selves
[transfer]: maturation-transfer.md
[questions]: maturation-transfer.md#what-changed-in-the-exodus-room
[switch]: maturation-transfer.md#read-the-model-on-each-side-of-a-decision
[wait]: desktop.md#when-an-activity-goes-quiet
[packet]: desktop.md#save-the-current-field-handoff-or-an-issue-record
[provider]: studio-platform-return.md
[recovery]: receiving-checks.md#provider-evidence-and-recovery
[buyer]: inverter-peer-return.md#what-the-buyers-files-support
[shift]: shift-intake.md#what-happened-while-nobody-was-listening
[fixed]: ../examples/exodus-design-room/fixed/
[exchange]: ../examples/exodus-design-room/fixed/#did-the-waiting-peers-receive-elis-messages
[reflow]: ../examples/ec1-energy-control/reflow/
[roles]: ../examples/ec1-energy-control/inspection/register/#who-supplied-what
[appeal]: ../examples/ec1-energy-control/appeal/
[visual]: ../examples/ec1-energy-control/exhibit/
[trial]: ../examples/owner-contributor/
[solo]: solar-inspection-nest.md
[runner]: ../src/neural/runner.rs
[context]: ../src/neural/context.rs
[store]: ../src/neural/store.rs
[membership]: ../src/neural/membership.rs
[files]: ../src/neural/issued_files.rs
[adopt]: ../src/neural/operations.rs
