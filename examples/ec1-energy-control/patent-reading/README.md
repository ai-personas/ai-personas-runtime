# What does the patent’s force plot actually predict?

**[Read Arden’s two-page note (PDF)](source-check/reading-note.pdf) · [Download the four values (CSV)](source-check/figure-23B.csv) · [Read the original brief](commission.txt)**

Open [US 11,511,891 B2, issued 29 November 2022 (PDF)](https://patentimages.storage.googleapis.com/b2/20/c9/debf1fe0819d2c/US11511891B2.pdf) to **PDF page 20**, drawing sheet **18 of 23**. Turn the view **90° right** to read Fig. 23B. Match the two *Poly.* entries in the line key to the heights at 40 kV; the point markers overlap the trendlines in the monochrome scan. The CSV is plain UTF-8, with a header and one row per curve and voltage. Its numbers are **predictions from the printed fits**, not a digitization of the observed points.

| Series as printed | Fit: force in µN, `x` in kV | At 30 kV | At 40 kV |
|---|---|---:|---:|
| Small Foam Thruster (lower at 40) | `0.2359x² − 9.8135x + 118.7` | 36.6050 µN | 103.6000 µN |
| Might Mouse1 (upper at 40) | `0.2914x² − 9.7267x + 80.887` | 51.3460 µN | 158.0590 µN |

The extra digits reproduce the substitution; they are not instrument precision. Neither fit is a pure, origin-constrained voltage-squared law. The **area comparison** comes from a different test: see Figs. 21–22 and “Multi-Blade Test Cases,” PDF p. 37, col. 24. At +25 kV the patent reports about 237 µN with 8 in² of grounded copper and an average of 421 µN with 15 in². The reported force ratio is about **1.77**, against about **1.8** computationally expected; **15/8 = 1.875** is the area ratio. These are not two readings from Fig. 23B.

This is the inventors’ account, not independent replication. Fig. 23B has no disconnect time, discharge history or load measurement. A static force on a stationary article does no mechanical work. Establishing net electrical generation would require signed energy in and out, including changes in storage and the cost of restoring the initial state. The [later Buhler presentation listing](https://www.youtube.com/watch?v=ApU4io78b4I) is a separate source: its description is not the lecture body. The listed English caption track returned no text on inspection; no transcript, lecture equations or raw observations were supplied. The [EC1 workshop](../) is a low-voltage accounting control, not a propulsion device or a demonstration of COP > 1.

### What came back from Studio

Source desk opened a new brief from the original research edition and invited only continuing Arden. The **only attachment** was the unchanged 42-page patent. Arden accepted, chose its methods, and stopped after twelve accepted direct Codex OAuth `gpt-6.2-astra` decisions at `high`. When text extraction failed and the PDF pages yielded no text, Arden rendered pages, read images of the figure and adjacent columns, and checked the layout of its own note. Two nonzero command returns were preserved and reached later decisions. There was no second invitation, human correction or peer contribution. This fork starts after event 425; its 153 inherited completions are not additional research calls.

Arden registered the CSV at #445. The first PDF output was **not** the final file: a later command produced the edition’s PDF (#447), followed by a visual check and registration (#448–449). Arden issued the pair publicly (#450), addressed the edition to the user (#451), and ran its own check with both files staged (#453–454). There was **no numbered dispatch**. The last choice closed the work and stopped (#455–457). This is an author-claimed completion, not a human verdict. All twelve decisions kept the original numerical character. Usage receipts reported 274,854 tokens and no monetary cost.

A separate receiver saved the Studio ZIP, compared it byte for byte with an independent export, and inspected both members without running any submitted code. Both PDF pages rendered on Letter paper with embedded fonts and no clipped text. The plot crop matched a separate rendering of the issued patent page; decimal substitution in both transcribed polynomials reproduced every CSV row. Neither the issuer check nor this digital receipt is a physical test.

| Exact file | Bytes | SHA-256 |
|---|---:|---|
| Patent attachment | 2,963,377 | `9063d905e548a1fae42aac68c08644fc773fbac7a34818586052c461d394f94d` |
| `source-check/reading-note.pdf` | 249,601 | `f9c06590134bd7b59385d613b33a1a3e2a26c63be09548424905895397095937` |
| `source-check/figure-23B.csv` | 641 | `f888e955506e21b0fc3221009f8808e93d2d09bdaf58c1b22fe0e7aa30e72bca` |

Edition: `delivery:delivery-5ec75b163a472626e1fb730064837181@1`. [How to turn a sheet in Studio](../../../docs/desktop.md#read-a-sideways-pdf-sheet).
