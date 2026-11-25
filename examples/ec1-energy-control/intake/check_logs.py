#!/usr/bin/env python3
"""Read-only intake for ideal EC1 synthetic exercises, 200 Hz, nominal SI values.
No inference from filenames. Ambiguous physical traces require human review.
"""
import argparse, csv, hashlib, io, json, math, sys
from pathlib import Path
from itertools import groupby

COLS=['t_s','in_V','store_V','load_V']
DT=.005
EPS=.000001  # ideal switch-state equality tolerance, V
EMPTY=.05     # near-empty endpoint, volts (1% of 5 V)
NAMES=['port_signed','port_inward','port_outward','R2_load','R1_heat']

def phase(r):
    t,v,s,l=r
    if abs(l)<=EPS:
        if v-s>EPS: return 'charge'
        if abs(v-s)<=EPS: return 'empty' if abs(s)<=EMPTY else 'isolation'
    if l>EPS and abs(v-s)<=EPS and abs(l-s)<=EPS: return 'discharge'
    return 'unresolved'

def powers(r):
    t,v,s,l=r; i=(v-s)/1000; p=v*i
    return p,max(p,0),max(-p,0),l*l/1000,1000*i*i

def interval(rows):
    e=[0.]*5
    for a,b in zip(rows,rows[1:]):
        dt=b[0]-a[0]
        for j,(x,y) in enumerate(zip(powers(a),powers(b))): e[j]+=0.5*dt*(x+y)*1000
    u0=.5*rows[0][2]**2; u1=.5*rows[-1][2]**2  # mJ for 1 mF
    q=dict(zip(NAMES,e)); q.update(C1_initial=u0,C1_final=u1,C1_change=u1-u0,load_minus_inward=e[3]-e[1],residual=e[0]-e[3]-e[4]-(u1-u0))
    return dict(span_s=[rows[0][0],rows[-1][0]],samples=len(rows),mJ=q)

def inspect(p):
    out={'file':p.name,'disposition':'HOLD'}
    try:
        raw=p.read_bytes(); out['sha256']=hashlib.sha256(raw).hexdigest()
        rd=csv.reader(io.StringIO(raw.decode('utf-8-sig'),newline=''),strict=True)
        if next(rd,None)!=COLS: raise ValueError('expected exact header '+','.join(COLS))
        rows=[]
        for row in rd:
            if len(row)!=4: raise ValueError('field count at CSV line '+str(rd.line_num))
            try: x=tuple(map(float,row))
            except ValueError: raise ValueError('invalid number at CSV line '+str(rd.line_num)) from None
            if not all(map(math.isfinite,x)): raise ValueError('nonfinite number at CSV line '+str(rd.line_num))
            if any(not -EMPTY<=y<=5+EMPTY for y in x[1:]): raise ValueError('outside low-voltage exercise envelope at CSV line '+str(rd.line_num))
            rows.append(x)
        if len(rows)<2: raise ValueError('need two or more samples')
        reasons=[]; phases=list(map(phase,rows)); runs=[]
        for k,g in groupby(range(len(rows)),key=phases.__getitem__):
            idx=list(g); runs.append({'phase':k,'span_s':[rows[idx[0]][0],rows[idx[-1]][0]],'samples':len(idx)})
        start=0; seg=[]; breaks=[]; dtmax=0; edges=0
        for i in range(1,len(rows)):
            a,b=rows[i-1:i+1]; dt=b[0]-a[0]; dtmax=max(dtmax,dt); ka,kb=phases[i-1:i+1]
            if dt<0 or not math.isfinite(dt): raise ValueError('invalid time order/span at sample '+str(i+1))
            if dt==0:
                if (ka,kb) not in [('empty','charge'),('charge','isolation'),('isolation','discharge')] or abs(a[2]-b[2])>EPS: raise ValueError('unexplained duplicate timestamp at sample '+str(i+1))
                edges+=1; continue
            if not math.isclose(dt,DT,rel_tol=.001,abs_tol=1e-9):
                seg.append(interval(rows[start:i])); start=i; breaks.append({'from_s':a[0],'to_s':b[0],'dt_s':dt}); continue
            if ka!=kb:
                seg.append(interval(rows[start:i])); start=i
                breaks.append({'from_s':a[0],'to_s':b[0],'dt_s':dt,'reason':'unwitnessed switch'})
                reasons.append('phase changed across positive time; switch not witnessed, interval excluded')
                continue
            # C dV/dt = (IN-STORE)/R1 - LOAD/R2; here both RCs = 1 s.
            # Check only same-state, observed sample pairs. Never synthesize data.
            if ka==kb and ka!='unresolved':
                predicted=.5*dt*((a[1]-a[2]-a[3])+(b[1]-b[2]-b[3]))
                if abs((b[2]-a[2])-predicted)>2e-6+.02*abs(predicted):
                    reasons.append('same-phase STORE change inconsistent with nominal RC current')
            if ka==kb=='charge' and b[2]<a[2]-EPS: reasons.append('STORE falls during charge')
            if ka==kb=='discharge' and b[2]>a[2]+EPS: reasons.append('STORE rises during discharge')
            if abs(b[2]-a[2])>5.1*dt+EPS: reasons.append('STORE slew inconsistent with nominal passive RC')
        if dtmax==0: raise ValueError('zero elapsed time')
        seg.append(interval(rows[start:])); out.update(samples=len(rows),runs=runs,zero_time_switches=edges,max_dt_s=dtmax,breaks=breaks,covered_intervals=seg)
        if any('reason' not in item for item in breaks): reasons.append('cadence break; excluded every off-cadence interval, no interpolation or cycle total')
        if abs(rows[0][2])>EMPTY: reasons.append('precharged start; charge input before record unknown')
        if [x['phase'] for x in runs]!=['empty','charge','isolation','discharge']: reasons.append('ordered empty, charge, isolation, discharge signature not fully observed')
        for run in runs:
            if run['phase']!='empty' and run['span_s'][1]<=run['span_s'][0]: reasons.append('no positive-time '+run['phase']+' run')
        if max(r[2] for r in rows)-rows[0][2]<=EMPTY: reasons.append('no resolved STORE rise from initial state')
        if abs(rows[-1][2])>EMPTY or abs(rows[-1][2]-rows[0][2])>EMPTY: reasons.append('STORE endpoints not both near empty and matched within 50 mV')
        out['reasons']=list(dict.fromkeys(reasons)); out['disposition']='HOLD' if reasons else 'USABLE_COMPLETE_SYNTHETIC_CYCLE'
    except (ValueError, OSError, csv.Error, OverflowError) as e: out['reasons']=[str(e)]
    return out

def main():
    ap=argparse.ArgumentParser(description=__doc__); ap.add_argument('directory',type=Path,help='inspect direct child *.csv files, case insensitive, without changing them'); args=ap.parse_args()
    try: files=sorted((p for p in args.directory.iterdir() if p.suffix.lower()=='.csv' and p.is_file()),key=lambda p:p.name)
    except OSError as e: ap.error(str(e))
    if not files: ap.error('no CSV files in directory')
    reports=[inspect(p) for p in files]; print(json.dumps(reports,indent=2,allow_nan=False)); return int(any(r['disposition']=='HOLD' for r in reports))
if __name__=='__main__': sys.exit(main())
