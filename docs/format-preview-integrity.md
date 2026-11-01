# Scientific and CAD preview integrity

The desktop's compatible renderers produce derived representations of a selected
file. They do not create a task deliverable, validate engineering correctness, or
establish that every representation of the source format is supported. Conversion
runs through the existing worker and cancellation path. The original file remains
unchanged; useful errors and limitations must remain distinguishable from values.

## HDF5 dataspaces and values

The [HDF5 adapter](../src/desktop/native_renderers.py) preserves the original
`shape`, `dtype`, attributes, and the distinction between null, scalar and simple
dataspaces. A null dataspace is a valid empty dataset, with `shape: null` and
`preview_status: empty_dataspace`. It is not a scalar value, a decoding failure,
or a reason to discard other datasets in the file. A zero-length simple array
retains its dimensions; a scalar retains `shape: []`.

Numeric arrays keep their rank and row/column relationships rather than being
flattened. The existing JSON table/plot viewer can inspect matrices and nested
arrays using their exact JSON Pointers. `preview_shape` and `preview_selection`
identify the leading hyperslab that was actually read. `truncated` and the
catalogue's `truncated_datasets` distinguish sampled data from complete values.
These fields establish representation and sample extent, not physical units,
statistical representativeness, simulation quality or artifact correctness.

The existing preview limits remain unchanged: at most 20,000 values per dataset,
256 datasets, 2,048 traversed group/dataset nodes, and 32 hierarchy levels. They
are preview limits, not persona-work or inference budgets. Broad large-file
performance and memory use have not been established by this implementation.
In particular, attribute size and group-key enumeration are not comprehensively
bounded by the value-count limit.

A dataset-specific read error is retained as `preview_status: read_error` with no
invented values. Readable sibling datasets remain available. Variable-length,
reference, compound and opaque values remain unsupported and carry an explicit
status. Array-valued attributes retain their list structure. Nonfinite JSON-native
floats become null; extended NumPy scalars without a Python-native scalar become
text rather than recursing indefinitely. Extended floating-point values are
labelled as text: numeric plotting can lose their precision.

External/soft links are not followed. Virtual datasets and external-storage
datasets receive `linked_data_unavailable`, without attempting to resolve their
source data. This also applies to virtual mappings within the selected file in
this adapter. HDF5 may return fill values when a virtual source is absent; those
must not be presented as observations of the absent source. Supporting such
mappings requires explicit source resolution and fidelity handling, not merely
showing a successful array read. Dimension scales, references and full HDF5 type
coverage remain separate unsupported features.

## FreeCAD stored geometry

The existing FCStd path reads stored BREP members, without loading feature scripts
or executing the FreeCAD document. If any selected BREP member fails decoding or
is empty, conversion fails explicitly instead of silently omitting that member
and displaying a potentially incomplete assembly as an ordinary preview.

Temporary BREP members are created inside the parent viewer's owned conversion
directory and removed after their use. Keeping them there allows the directory's
existing lifecycle to remove leftovers after process-group cancellation. This is
not a claim that every native lifecycle/platform path has been executed.

Valid stored shapes are still combined and tessellated. The result does not
preserve visible-object selection, feature history, materials, assembly semantics,
or engineering validity; intermediate stored shapes can overlap. STEP/IGES and
DXF conversion routes otherwise retain their existing behavior and limitations.
