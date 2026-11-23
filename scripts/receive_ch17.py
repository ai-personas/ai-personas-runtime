"""Independent dimensional receiving check for the two CH-17 rev A rail cut contours.

python3 receive_ch17.py SHOP.zip [--left MEMBER[#id|#@n]] [--right MEMBER[#id|#@n]]
0 = measured and passes; 1 = measured and fails; 2 = cannot identify/measure the cuts.
Python standard library only. Reads the supplied ZIP in place; never runs its contents.
"""
import argparse
from decimal import Decimal as D
from hashlib import sha256
import io
from itertools import product
import json
import math
from pathlib import Path, PurePosixPath
import re
from xml.etree import ElementTree as ET
from zipfile import BadZipFile, ZipFile


class Unmeasurable(ValueError):
    pass


def need(ok, msg):
    if not ok:
        raise Unmeasurable(msg)


NUM = r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?"
UNITS = {"":1., "px":1., "mm":96/25.4, "cm":96/2.54, "in":96., "pt":96/72, "pc":16.}
IDENTITY = (1.,0.,0.,1.,0.,0.)
EPS = 1e-8  # mm; topology only, not an extra supplier tolerance
SVG = "{http://www.w3.org/2000/svg}"
HASH = lambda b: sha256(b).hexdigest()


def tokens(text, pattern):
    regex = re.compile(pattern)
    out, end = [], 0
    for m in regex.finditer(text):
        need(not text[end:m.start()].strip(" \n\r\t,"), f"unsupported SVG syntax near {text[end:end+24]!r}")
        out.append(m.group()); end = m.end()
    need(not text[end:].strip(" \n\r\t,"), "unsupported trailing SVG syntax")
    return out


def number(x):
    v = float(x)
    need(math.isfinite(v), "nonfinite SVG number")
    return v


def numbers(text):
    return [number(x) for x in tokens(text,NUM)]


def length(text):
    m = re.fullmatch(rf"({NUM})(px|mm|cm|in|pt|pc)?", text.strip())
    need(m, f"unresolved SVG length {text!r}")
    value = number(m[1])*UNITS[m[2] or ""]
    need(math.isfinite(value), "nonfinite physical length")
    return value


def compose(A,B):
    a,b,c,d,e,f=A; g,h,i,j,k,l=B
    return a*g+c*h,b*g+d*h,a*i+c*j,b*i+d*j,a*k+c*l+e,b*k+d*l+f


def transform(text):
    m, end = IDENTITY, 0
    for f in re.finditer(r"([A-Za-z]+)\s*\(([^()]*)\)", text):
        need(not text[end:f.start()].strip(" ,\t\n\r"), "invalid transform list")
        end=f.end(); name=f[1]; p=numbers(f[2])
        if name=="matrix" and len(p)==6: t=p
        elif name=="translate" and len(p) in (1,2): t=(1,0,0,1,p[0],p[1] if len(p)==2 else 0)
        elif name=="scale" and len(p) in (1,2): t=(p[0],0,0,p[-1],0,0)
        elif name=="rotate" and len(p) in (1,3):
            c,s=math.cos(math.radians(p[0])),math.sin(math.radians(p[0]))
            x,y=p[1:] if len(p)==3 else (0,0)
            t=(c,s,-s,c,x-c*x+s*y,y-s*x-c*y)
        elif name in ("skewX","skewY") and len(p)==1:
            v=math.tan(math.radians(p[0]));t=(1,v if name=="skewY" else 0,v if name=="skewX" else 0,1,0,0)
        else: raise Unmeasurable(f"unsupported transform {name}")
        m=compose(m,t)
    need(not text[end:].strip(" ,\t\n\r"), "invalid trailing transform")
    return m


def viewport(root):
    width,height=(length(root.get(k,""))*25.4/96 for k in ("width","height"))
    need(min(width,height)>0, "root needs a physical viewport")
    box=root.get("viewBox")
    if box is None: return (width,height),(25.4/96,0,0,25.4/96,0,0)
    b=numbers(box);need(len(b)==4 and min(b[2:])>0,"invalid viewBox")
    x,y,w,h=b;sx,sy=width/w,height/h
    aspect=root.get("preserveAspectRatio","xMidYMid meet").split()
    if aspect==["none"]: return (width,height),(sx,0,0,sy,-x*sx,-y*sy)
    need(len(aspect) in (1,2) and re.fullmatch(r"x(Min|Mid|Max)Y(Min|Mid|Max)",aspect[0])
         and (len(aspect)==1 or aspect[1] in ("meet","slice")), "unsupported preserveAspectRatio")
    scale=(max if aspect[-1]=="slice" else min)(sx,sy)
    factor={"Min":0,"Mid":0.5,"Max":1}
    return (width,height),(scale,0,0,scale,factor[aspect[0][1:4]]*(width-w*scale)-x*scale,
                          factor[aspect[0][5:]]*(height-h*scale)-y*scale)


def line_paths(d):
    """SVG moveto, lineto, horizontal/vertical lineto and close, including relative segments."""
    words=tokens(d,rf"(?:{NUM}|[A-Za-z])")
    paths, current, cmd, x, y, i = [], [], None, 0., 0., 0
    while i<len(words):
        if words[i].isalpha():
            cmd=words[i];i+=1
            need(cmd in "MLHVZmlhvz", f"not a linear cut path: {cmd}")
            if cmd.upper()=="Z":
                need(current,"close without a cut path")
                x,y=current[0];current.append((x,y));paths.append(current);current=[];cmd=None
                continue
        need(cmd and (current or cmd.upper()=="M"), "missing SVG moveto")
        k=cmd.upper(); n=2 if k in ("M","L") else 1
        need(i+n<=len(words) and not any(v.isalpha() for v in words[i:i+n]),"incomplete path coordinate")
        p=[number(v) for v in words[i:i+n]];i+=n
        if k=="M":
            if current: paths.append(current)
            current=[]
        if k in ("M","L","H"): x=p[0]+(x if cmd.islower() else 0.)
        if k in ("M","L","V"): y=p[-1]+(y if cmd.islower() else 0.)
        current.append((x,y))
        if k=="M": cmd="l" if cmd=="m" else "L"
    if current: paths.append(current)
    need(paths,"empty SVG path")
    return paths


def outlines(node, tag):
    if tag=="rect":
        need(all(length(node.get(k,"0"))==0 for k in ("rx","ry")), "rounded rectangle requires separate review")
        x,y,w,h=[length(node.get(k,"0")) for k in ("x","y","width","height")]
        need(min(w,h)>0, "degenerate rectangle")
        return [[(x,y),(x+w,y),(x+w,y+h),(x,y+h),(x,y)]]
    if tag=="path": return line_paths(node.get("d",""))
    p=numbers(node.get("points",""));need(len(p)>=6 and len(p)%2==0,"invalid polygon/polyline")
    vertices=list(zip(p[::2],p[1::2]))
    return [vertices+([vertices[0]] if tag=="polygon" else [])]


def rectangle(points, page):
    need(all(math.isfinite(x) and math.isfinite(y) and -EPS<=x<=page[0]+EPS and -EPS<=y<=page[1]+EPS
             for x,y in points), "contour falls outside its physical viewport")
    need(len(points)>=4 and math.dist(points[0],points[-1])<=EPS, "cut contour is open")
    v=[]
    for p in points[:-1]:
        if not v or math.dist(v[-1],p)>EPS: v.append(p)
    if len(v)>1 and math.dist(v[-1],v[0])<=EPS: v.pop()
    sub=lambda a,b:(a[0]-b[0],a[1]-b[1])
    dot=lambda a,b:a[0]*b[0]+a[1]*b[1]
    cross=lambda a,b:a[0]*b[1]-a[1]*b[0]
    # Merge subdivisions of a straight side; never discard a reversal or a notch.
    while len(v)>4:
        for i in range(len(v)):
            a,b=sub(v[i],v[i-1]),sub(v[(i+1)%len(v)],v[i])
            if abs(cross(a,b))<=1e-11*math.hypot(*a)*math.hypot(*b) and dot(a,b)>0:
                v.pop(i);break
        else: break
    need(len(v)==4, "contour does not have four straight sides")
    vectors=[sub(v[(i+1)%4],v[i]) for i in range(4)]
    lengths=[math.hypot(*p) for p in vectors]
    need(min(lengths)>EPS and all(abs(dot(p,vectors[(i+1)%4]))<=1e-9*lengths[i]*lengths[(i+1)%4]
         for i,p in enumerate(vectors)) and abs(lengths[0]-lengths[2])<=EPS and abs(lengths[1]-lengths[3])<=EPS,
         "contour is not a rectangle")
    turns=[cross(vectors[i],vectors[(i+1)%4]) for i in range(4)]
    need(min(turns)>0 or max(turns)<0, "crossing or retraced cut")
    width,long=sorted([(lengths[0]+lengths[2])/2,(lengths[1]+lengths[3])/2])
    # Remove only sub-picometre transform noise. Fit checks use Decimal, not display rounding.
    mm=lambda value:D(format(value,".12f")).normalize()
    return dict(width_mm=mm(width),length_mm=mm(long),corners_mm=[[mm(x),mm(y)] for x,y in v])


def contours(data):
    need(b"<!DOCTYPE" not in data.upper() and b"<!ENTITY" not in data.upper() and b"<?xml-stylesheet" not in data.lower(),
         "not a self-contained static cut SVG")
    root=ET.fromstring(data)
    need(root.tag in (SVG+"svg","svg"),"not an SVG document")
    page, m=viewport(root)
    need(not root.get("transform"),"root transform requires separate review")
    need(not any(n.tag in ("style",SVG+"style","script",SVG+"script") for n in root.iter()),"stylesheet or script requires separate review")
    result=[]
    presentation={"fill","fill-rule","fill-opacity","stroke","stroke-width","stroke-opacity","stroke-linecap",
                  "stroke-linejoin","stroke-miterlimit","vector-effect","color","opacity","display","visibility",
                  "stroke-dasharray","stroke-dashoffset","paint-order","shape-rendering"}
    def walk(n, matrix, parents, inherited):
        tag=n.tag.removeprefix(SVG)
        if tag in ("title","desc","metadata","defs","{http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd}namedview"): return
        need(tag in ("g","rect","path","polygon","polyline") or n is root,
             f"unsupported artwork in selected cut member: {tag}")
        need(not any(n.get(k,"none")!="none" for k in ("clip-path","mask","filter","marker-start","marker-mid","marker-end")),
             "clipped, filtered or marked vectors require separate review")
        props=dict(inherited);props.update({k:v.strip() for k,v in n.attrib.items() if k in presentation})
        for item in filter(str.strip,n.get("style","").split(";")):
            k,sep,v=item.partition(":");k=k.strip()
            need(sep and k in presentation,"unsupported CSS in selected cut member")
            props[k]=v.strip()
        need(props.get("display")!="none" and props.get("visibility","visible")=="visible" and
             props.get("stroke-dasharray","none")=="none","hidden or dashed artwork requires separate review")
        need(float(props.get("opacity","1"))>0 and not any("url(" in v for v in props.values()),
             "invisible or externally painted artwork")
        mx=compose(matrix,transform(n.get("transform","")));ids=parents+([n.get("id")] if n.get("id") else [])
        if tag in ("svg","g"):
            for child in n: walk(child,mx,ids,props)
        else:
            visible=lambda paint, default: props.get(paint,default) not in ("none","transparent") and float(props.get(paint+"-opacity","1"))>0
            need(visible("fill","black") or (visible("stroke","none") and length(props.get("stroke-width","1"))>0), "cut contour has no visible paint")
            a,b,c,d,e,f=mx
            for path in outlines(n,tag):
                v=rectangle([(a*x+c*y+e,b*x+d*y+f) for x,y in path],page)
                result.append(dict(index=len(result)+1,ids=ids,**v))
    walk(root,m,[],{})
    need(result,"no measurable rail contours")
    return result


def score(side, geo):
    width,long=geo["width_mm"],geo["length_mm"]
    plate, band, setback = D("96.00"),D("87.00"),D("4.80")
    cut=D("0.10"); ink=D("0.15"); bearing=D("2.90" if side=="left" else "4.00")
    border=lambda w,b,r: w-b-r if side=="left" else r
    nominal=border(plate,band,setback)
    def extremes(center,tol): return (center-D(tol),center+D(tol))
    corners=product(extremes(plate,"0.35"),extremes(band,"0.30"),extremes(setback,"0.25"),(-cut,cut))
    gaps=[(border(p,b,r)-(width+delta),p,b,r,width+delta) for p,b,r,delta in corners]
    gap,p,b,r,w=min(gaps)
    checks={"drawn_length_52mm":long==D("52"),"minimum_bearing":width-cut>=bearing,"minimum_ink_clearance":gap>=ink}
    return dict(geometry=geo,nominal_border_mm=nominal,nominal_clearance_mm=nominal-width,
                nominal_fit=width>=bearing and nominal-width>=ink and checks["drawn_length_52mm"],
                required_bearing_mm=bearing,finished_width_range_mm=[width-cut,width+cut],least_bearing_mm=width-cut,
                required_clearance_mm=ink,least_clearance_mm=gap,clearance_surplus_mm=gap-ink,
                limiting_clearance_case_mm=dict(plate_width=p,band_width=b,right_setback=r,rail_finished_width=w,
                                                available_border=border(p,b,r)),
                checks=checks,passed=all(checks.values()))


def inspect(data, left=None, right=None):
    base=dict(drawing="CH-17 rev A",scope="Digital dimensions of the designated finished rail cut contours",
              zip_sha256=HASH(data),zip_bytes=len(data))
    try:
        with ZipFile(io.BytesIO(data)) as z:
            names=z.namelist()
            need(len(set(names))==len(names),"duplicate ZIP member name")
            svgs=[n for n in names if n.lower().endswith(".svg")]
            files, digests, used, sides={},{},set(),{}
            for side,selector in (("left",left),("right",right)):
                if selector is None:
                    matches=[n for n in svgs if side in re.split(r"[^a-z]+",PurePosixPath(n).stem.lower())]
                    need(len(matches)==1,f"select the {side} member from the fitting note with --{side}")
                    member,part=matches[0],""
                else:
                    member,sep,part=selector.partition("#")
                    need(not sep or part,"empty contour selector")
                need(member in svgs,f"missing SVG member: {member}")
                if member not in files:
                    content=z.read(member); files[member]=contours(content); digests[member]=HASH(content)
                candidates=files[member]
                if part.startswith("@"): candidates=[v for v in candidates if part=="@"+str(v["index"])]
                elif part: candidates=[v for v in candidates if part in v["ids"]]
                need(len(candidates)==1,f"{side}: identify one contour in {member} by #id or #@number")
                item=candidates[0];key=(member,item["index"])
                need(key not in used,"same cut selected twice")
                used.add(key);sides[side]=score(side,dict(member=member,member_sha256=digests[member],**item))
            need(len(used)==sum(map(len,files.values())), "additional contours in selected cut members")
        passed=all(v["passed"] for v in sides.values())
        return dict(**base,status="pass" if passed else "fail",sides=sides,
                    unselected_svg_members=sorted(set(svgs)-files.keys()))
    except (BadZipFile,ET.ParseError,ValueError,ArithmeticError,RuntimeError,NotImplementedError) as error:
        return dict(**base,status="unmeasurable",reason=str(error))


def json_value(v):
    if isinstance(v,D): return format(v,"f")
    raise TypeError(type(v).__name__)


if __name__=="__main__":
    cli=argparse.ArgumentParser(description=__doc__,formatter_class=argparse.RawDescriptionHelpFormatter)
    cli.add_argument("archive",type=Path)
    for side in ("left","right"): cli.add_argument("--"+side,metavar="MEMBER[#id|#@n]")
    args=cli.parse_args()
    report=inspect(args.archive.read_bytes(),args.left,args.right)
    print(json.dumps(report,sort_keys=True,indent=2,default=json_value))
    raise SystemExit({"pass":0,"fail":1,"unmeasurable":2}[report["status"]])
