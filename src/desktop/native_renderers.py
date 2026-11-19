#!/usr/bin/env python3
"""Trusted read-only render adapters. Never imports or executes artifact scripts."""
import argparse, json, math, os, sys, tempfile, zipfile
from pathlib import Path

def dxf(src, dst):
    import ezdxf
    from ezdxf.addons.drawing import Frontend, RenderContext, layout
    from ezdxf.addons.drawing.svg import SVGBackend
    doc=ezdxf.readfile(str(src)); backend=SVGBackend()
    Frontend(RenderContext(doc),backend).draw_layout(doc.modelspace(),finalize=True)
    dst.write_text(backend.get_string(layout.Page(297,210,units=layout.Units.mm),
        settings=layout.Settings(fit_page=True,output_layers=True)),encoding="utf-8")
    return {"kind":"svg","entities":sum(1 for _ in doc.modelspace()),
        "note":"DXF modelspace only; paper layouts, external references, proxy entities and CAD constraints are not represented."}

def cad(src,dst):
    from OCP.BRep import BRep_Builder
    from OCP.BRepTools import BRepTools
    from OCP.TopoDS import TopoDS_Shape
    from OCP.STEPControl import STEPControl_Reader
    from OCP.IGESControl import IGESControl_Reader
    ext=src.suffix.lower()
    if ext in (".step",".stp",".iges",".igs"):
        reader=STEPControl_Reader() if ext in (".step",".stp") else IGESControl_Reader()
        status=reader.ReadFile(str(src))
        if int(status)!=1: raise ValueError("Open CASCADE rejected the CAD source (status {}).".format(int(status)))
        if reader.TransferRoots()==0: raise ValueError("No transferable geometry roots.")
        shape=reader.OneShape(); kind="STEP" if ext in (".step",".stp") else "IGES"
    elif ext==".fcstd":
        with zipfile.ZipFile(src) as z:
            members=z.infolist()
            infos=sorted((i for i in members if i.filename.lower().endswith((".brp",".brep"))),key=lambda i:i.filename)
            if not infos: raise ValueError("FreeCAD archive has no stored BREP shape members.")
            shapes=[]
            for info in infos:
                # The parent viewer owns this output directory. Keeping extracted
                # members here also lets it clean up after converter cancellation.
                with tempfile.NamedTemporaryFile(suffix=".brep",dir=dst.parent,delete=False) as tmp:
                    tmpname=tmp.name
                    try:
                        tmp.write(z.read(info))
                    except Exception:
                        os.unlink(tmpname)
                        raise
                try:
                    sh=TopoDS_Shape()
                    if not BRepTools.Read_s(sh,tmpname,BRep_Builder()) or sh.IsNull():
                        raise ValueError("A stored FreeCAD BREP member could not be decoded; no partial assembly preview was produced.")
                    shapes.append(sh)
                finally: os.unlink(tmpname)
        if not shapes: raise ValueError("No valid stored BREP geometry found.")
        if len(shapes)==1: shape=shapes[0]
        else:
            from OCP.TopoDS import TopoDS_Compound
            shape=TopoDS_Compound(); builder=BRep_Builder(); builder.MakeCompound(shape)
            for sh in shapes: builder.Add(shape,sh)
        kind="FreeCAD stored BREP"
    else: raise ValueError("Unsupported CAD source extension.")
    if shape.IsNull(): raise ValueError("CAD source produced an empty shape.")
    from OCP.BRepMesh import BRepMesh_IncrementalMesh
    from OCP.StlAPI import StlAPI_Writer
    mesher=BRepMesh_IncrementalMesh(shape,0.15,False,0.35,True); mesher.Perform()
    if not mesher.IsDone(): raise ValueError("Open CASCADE tessellation failed.")
    writer=StlAPI_Writer(); writer.ASCIIMode=False
    if not writer.Write(shape,str(dst)): raise ValueError("Could not write STL preview.")
    return {"kind":"mesh","source_kind":kind,
        "note":"{} decoded with Open CASCADE and tessellated (0.15 model-unit linear deflection). PMI, colors, constraints, parametric history and assembly semantics are not preserved.".format(kind)}

def clean(v):
    if isinstance(v,bytes): return v.decode("utf-8",errors="replace")
    if hasattr(v,"item"):
        try:
            scalar=v.item()
            # Extended NumPy scalar types can return another scalar of the
            # same type. Keep their textual value instead of recursing forever.
            if type(scalar) is not type(v): return clean(scalar)
        except (ValueError,TypeError): pass
    if isinstance(v,(list,tuple)): return [clean(x) for x in v]
    if isinstance(v,float) and not math.isfinite(v): return None
    if v is None or isinstance(v,(str,int,float,bool)): return v
    return str(v)

def hdf5(src,dst):
    import h5py, numpy as np
    datasets={}; links={}

    def attributes(obj):
        # Attributes remain metadata; arrays should not turn into opaque NumPy
        # display strings. No referenced objects or linked files are opened.
        return {str(key):clean(value.tolist() if isinstance(value,np.ndarray) else value)
                for key,value in obj.attrs.items()}

    def preview(obj):
        shape=None if obj.shape is None else list(obj.shape)
        entry={"dtype":str(obj.dtype),"shape":shape,
               "dataspace":"null" if shape is None else "scalar" if not shape else "simple"}
        try:
            entry["attributes"]=attributes(obj)
        except (OSError,ValueError,TypeError):
            entry["attributes_error"]="Attributes could not be decoded; dataset values are considered separately."
        if shape is None:
            # HDF5 null dataspaces have no values; they are not scalar datasets
            # and must not prevent inspection of other datasets in this file.
            entry.update(preview_status="empty_dataspace",data=None)
            return entry
        if obj.is_virtual or obj.external:
            # Missing VDS sources may legitimately read as fill values. Do not
            # present those as observed source data in a selected-file preview.
            entry.update(preview_status="linked_data_unavailable",
                         preview_error="Virtual mappings or external storage require source resolution; this preview does not resolve those sources.")
            return entry
        if obj.dtype.kind in ("O","V"):
            entry.update(preview_status="unsupported_dtype",
                         preview_error="Variable-length, reference, compound or opaque values are not converted.")
            return entry
        arr=np.asarray(obj[()])
        # Preserve rank and row/column relationships for the native JSON table
        # viewer. In particular, a matrix must not become one flattened column.
        if obj.dtype.kind == "f" and obj.dtype.itemsize > 8:
            entry["value_representation"]="Extended-precision values are text; numeric plots can lose precision."
        entry.update(preview_status="complete_values",
                     preview_shape=list(arr.shape),
                     preview_selection={"start":[0]*len(shape),"stop":shape,"step":[1]*len(shape)},
                     data=clean(arr.tolist()))
        return entry

    with h5py.File(src,"r") as h:
        # An iterative walk has no application depth limit. Object identity
        # prevents actual hard-link cycles without hiding the alias path.
        visited={}; pending=[(h, "")]
        while pending:
            group,prefix=pending.pop()
            address=h5py.h5o.get_info(group.id).addr
            if address in visited:
                links[prefix]={"kind":"hard_group_alias","target":visited[address] or "/"}
                continue
            visited[address]=prefix
            for key in sorted(group.keys()):
                name=(prefix+"/"+key).lstrip("/")
                link=group.get(key,getlink=True)
                if not isinstance(link,h5py.HardLink):
                    links[name]={"kind":type(link).__name__,"target":getattr(link,"path",None),
                                 "file":getattr(link,"filename",None),"status":"source_not_resolved"}
                    continue
                obj=group.get(key)
                if isinstance(obj,h5py.Group): pending.append((obj,name)); continue
                if not isinstance(obj,h5py.Dataset): continue
                try:
                    entry=preview(obj)
                except (OSError,ValueError,TypeError):
                    # An unsupported/unreadable dataset must not erase valid
                    # siblings. It gets no fabricated values or success label.
                    entry={"preview_status":"read_error",
                           "preview_error":"Dataset metadata or values could not be decoded."}
                datasets[name]=entry
        try:
            attrs=attributes(h); attrs_error=None
        except (OSError,ValueError,TypeError):
            attrs={}; attrs_error="File attributes could not be decoded."
    payload={"format":"HDF5","file_attributes":attrs,"datasets":datasets,"links":links,
             "coverage":{"dataset_count":len(datasets),"unique_groups":len(visited)}}
    if attrs_error is not None: payload["file_attributes_error"]=attrs_error
    dst.write_text(json.dumps(payload,ensure_ascii=False,allow_nan=False),encoding="utf-8")
    return {"kind":"json","dataset_count":len(datasets),
        "note":"Readable supported datasets retain all values and dimensions, without application size, count or depth caps. Null dataspaces, unavailable linked data and per-dataset errors are explicit. External/soft links and virtual/external-storage sources are not resolved; link targets and hard-group aliases are listed. Nonfinite JSON floats become null, and extended scalar types may be text. Dimension scales and references are not resolved."}

def main():
    p=argparse.ArgumentParser(); p.add_argument("mode",choices=("dxf","cad","hdf5")); p.add_argument("source",type=Path); p.add_argument("target",type=Path); a=p.parse_args()
    if not a.source.is_file(): p.error("source is not a regular file")
    a.target.parent.mkdir(parents=True,exist_ok=True)
    try:
        result={"dxf":dxf,"cad":cad,"hdf5":hdf5}[a.mode](a.source,a.target)
        print(json.dumps(result,ensure_ascii=False)); return 0
    except Exception as e:
        print("{}: {}".format(type(e).__name__,e),file=sys.stderr); return 2
if __name__=="__main__": raise SystemExit(main())
