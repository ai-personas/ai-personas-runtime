# Receiving note — inverter engineering review v4

Package checked: Arden’s “Low-voltage inverter — editable engineering review edition v4”, delivery:delivery-81c93f401a69b2a96d528b42f61541a4@1.

**Result: readable by stock KiCad 9, but not a clean library-backed recheck as delivered.** On the configured receiving toolchain, `kicad-cli --version` returned **9.0.9**. PDF and netlist exports of `design/inverter.kicad_sch` both succeeded (exit 0). This demonstrates schematic loading through the CLI; an interactive GUI opening/editing session was not tested.

ERC completed and reported **0 errors and 21 warnings**. All 21 are `lib_symbol_issues`: symbols were not found in their named libraries, covering Connector_Generic, Timer, Device, 4xxx, Driver_Motor, Regulator_Linear and power. A zero command exit status therefore does not mean a clean ERC result.

The delivered symbol-library table uses absolute `/tmp/personas-local-gYTLLz/installed/stock/symbols/...` paths. These are not portable shop-installation references. The referenced files existed during the inspection, so this is not a finding that those files were absent; the observed failure is unresolved symbol lookup.

**Issuer action:** correct the package’s library bindings for stock KiCad 9, using portable stock-library references or appropriately packaged project-local libraries. Preserve the design, verify the exact symbol identifiers resolve against the intended libraries, rerun ERC on a clean receiving installation, and reissue the corrected package with the recheck report. Successful export alone is insufficient to close these warnings.

No design changes were made and no order was placed. This is a receiving compatibility finding, not electrical-design approval or fabrication authorization.
