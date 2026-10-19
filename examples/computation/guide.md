# Check and reuse an integer-sum tool

This is operator-supplied procedural guidance, not persona-defining prose or an
execution grant. Import this guide and `sum-u32.wat` as separate supporting assets
with their actual authorship and access policies. Locate their accepted exact
references through the runtime's bounded asset discovery/read interface.

Before relying on this tool, inspect its scope and choose a small known-answer
case. The program sums unsigned decimal integers from 0 to 4294967295, separated
by ASCII spaces, commas, tabs or newlines. Repeated delimiters are harmless; an
empty list returns zero. Negative values, fractions, non-ASCII text, unsupported
characters and out-of-range values fail. It does no unit conversion or domain
validation. Check that summing the values is appropriate to the request.

Read or select this complete guide. If an operator granted this persona/work
computation using this exact guide, a fresh request exposes
`CONTROL.computation_capabilities`. Copy the supplied capability and binding;
the engine accepts no guessed or remembered binding. Invoke `effect` with the
exact full UTF-8 program reference and the exact input string. The tool is a
WebAssembly program with no imports, filesystem, network or model access.

Wait for a fresh decision with the host's `computation_result`. Confirm success
and compare the actual output to the known-answer case. A proposed call or a
persona's own predicted answer is not a successful test. If the test fails or
the result is uncertain, report the observed problem and preserve its limits.
Do not claim the requested calculation was executed.

Once that check succeeds, use the same exact program for the requested input
within the remaining grant and work allowance. Inspect the fresh actual result,
check its plausibility, and report the input, output, exact program reference and
limits. A saved result may be a task asset without a characteristic refinement.
Refine personal guidance only if you judge that useful. Successful execution
does not establish independent assurance, source-data accuracy or acceptance.
