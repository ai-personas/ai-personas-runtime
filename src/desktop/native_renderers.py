#!/usr/bin/env python3
"""Trusted read-only render adapters. Never imports or executes artifact scripts."""
import argparse, json, math, os, sys, tempfile, zipfile
from pathlib import Path
MAX_VALUES, MAX_DATASETS = 20000, 256
MAX_GROUP_NODES = 2048
MAX_CAD_BYTES = 512 * 1024 * 1024
MAX_FCSTD_BREP_BYTES = 256 * 1024 * 1024

def dxf(src, dst):
    if src.stat().st_size > 256 * 1024 * 1024: raise ValueError("DXF exceeds the 256 MiB preview limit.")
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
    if src.stat().st_size > MAX_CAD_BYTES: raise ValueError("CAD source exceeds the 512 MiB preview limit.")
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
            if len(members)>10000: raise ValueError("FreeCAD archive exceeds the 10,000-entry safety limit.")
            infos=sorted((i for i in members if i.filename.lower().endswith((".brp",".brep")) and ".." not in Path(i.filename).parts),key=lambda i:i.filename)
            if not infos: raise ValueError("FreeCAD archive has no stored BREP shape members.")
            if len(infos)>MAX_DATASETS: raise ValueError("FreeCAD archive has more than 256 stored BREP members.")
            if sum(i.file_size for i in infos)>MAX_FCSTD_BREP_BYTES or any(i.file_size>64*1024*1024 for i in infos):
                raise ValueError("FreeCAD stored BREP members exceed the preview extraction limits.")
            shapes=[]
            for info in infos:
                with tempfile.NamedTemporaryFile(suffix=".brep",delete=False) as tmp:
                    tmp.write(z.read(info)); tmpname=tmp.name
                try:
                    sh=TopoDS_Shape()
                    if BRepTools.Read_s(sh,tmpname,BRep_Builder()) and not sh.IsNull(): shapes.append(sh)
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
        try: return clean(v.item())
        except (ValueError,TypeError): pass
    if isinstance(v,(list,tuple)): return [clean(x) for x in v]
    if isinstance(v,float) and not math.isfinite(v): return None
    if v is None or isinstance(v,(str,int,float,bool)): return v
    return str(v)

def hdf5(src,dst):
    import h5py, numpy as np
    datasets={}; truncated=[]
    with h5py.File(src,"r") as h:
        visited=set(); nodes=0; walk_limited=False
        def walk(group,prefix="",depth=0):
            nonlocal nodes, walk_limited
            if depth>32 or nodes>=MAX_GROUP_NODES:
                walk_limited=True; return
            address=h5py.h5o.get_info(group.id).addr
            if address in visited: return
            visited.add(address); nodes+=1
            for key in sorted(group.keys()):
                if len(datasets)>=MAX_DATASETS or nodes>=MAX_GROUP_NODES:
                    walk_limited=True; return
                if not isinstance(group.get(key,getlink=True),h5py.HardLink): continue
                obj=group.get(key); name=(prefix+"/"+key).lstrip("/")
                if isinstance(obj,h5py.Group): walk(obj,name,depth+1); continue
                if not isinstance(obj,h5py.Dataset): continue
                nodes+=1
                if obj.dtype.kind in ("O","V"):
                    datasets[name]={"dtype":str(obj.dtype),"shape":list(obj.shape),"preview_error":"Variable-length or opaque data not converted."}; continue
                shape=list(obj.shape)
                if not shape: arr=np.asarray(obj[()])
                else:
                    dims=shape[:]
                    while math.prod(dims)>MAX_VALUES:
                        axis=max(i for i,n in enumerate(dims) if n>1)
                        dims[axis]=max(1,dims[axis]//math.ceil(math.prod(dims)/MAX_VALUES))
                    arr=np.asarray(obj[tuple(slice(0,n) for n in dims)]).reshape(-1)[:MAX_VALUES]
                    if math.prod(shape)>MAX_VALUES: truncated.append(name)
                datasets[name]={"dtype":str(obj.dtype),"shape":shape,"data":clean(arr.tolist()),
                    "attributes":{str(k):clean(v) for k,v in obj.attrs.items()}}
        walk(h)
        attrs={str(k):clean(v) for k,v in h.attrs.items()}
    payload={"format":"HDF5","file_attributes":attrs,"datasets":datasets,
        "preview_limits":{"max_datasets":MAX_DATASETS,"max_values_per_dataset":MAX_VALUES,"max_group_nodes":MAX_GROUP_NODES,"max_group_depth":32,"group_walk_limited":walk_limited,"truncated_datasets":truncated}}
    dst.write_text(json.dumps(payload,ensure_ascii=False,allow_nan=False),encoding="utf-8")
    return {"kind":"json","dataset_count":len(datasets),
        "note":"Dataset values are bounded previews for table/plot inspection. Chunking, compression, dimension scales, references and values beyond limits are not represented. External/soft links are not followed."}

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
