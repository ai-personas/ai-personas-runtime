"""EC1 receiving desk | identify complete files without opening them as archives."""
import hashlib
import os
import stat
import sys
from pathlib import Path

ISSUED = {(139033, 'b546d9ef762dc6a457bfdbc74da0699a384b325ccaa73f30c427e941f287fbaa'): 'EARLIER', (139400, 'e06e81d9c1f96c33e9a009ac4ea7ce19dd7f1bfc003403ea8a9b2be69d463936'): 'R2'}

def scan(root):
    totals = {'R2':0, 'EARLIER':0, 'UNKNOWN':0, 'UNCHECKED':0}
    def mark(label, path, note):
        totals[label] += 1
        print(label, ascii(str(path)), note, sep=' | ')
    def error(e):
        mark('UNCHECKED', e.filename or root, 'cannot list directory; hold')
    def link(s):
        return stat.S_ISLNK(s.st_mode) or bool(getattr(s,'st_file_attributes',0) & 1024)
    try:
        start = root.lstat()
        if link(start) or not stat.S_ISDIR(start.st_mode):
            raise ValueError('select a real folder, not a file or link')
    except (OSError, ValueError) as e:
        print('STOP: folder unavailable:', ascii(str(e))); return 2
    for folder, dirs, files in os.walk(root, onerror=error, followlinks=False):
        dirs.sort(); files.sort()
        for name in list(dirs):
            path = Path(folder)/name
            try:
                if not link(path.lstat()): continue
            except OSError: pass
            dirs.remove(name); mark('UNCHECKED',path,'directory link or inaccessible; not followed')
        for name in files:
            path = Path(folder)/name
            try:
                s = path.lstat()
                if link(s) or not stat.S_ISREG(s.st_mode):
                    mark('UNCHECKED', path,'not a regular non-link file; hold'); continue
                h = hashlib.sha256(); n = 0
                with path.open('rb') as f:
                    for block in iter(lambda:f.read(1024*1024),b''):
                        h.update(block); n += len(block)
                t = path.lstat()
                if (s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns) != (t.st_dev,t.st_ino,n,t.st_mtime_ns) or t.st_size != n:
                    mark('UNCHECKED',path,'changed during read; hold'); continue
                digest = h.hexdigest(); label = ISSUED.get((n,digest),'UNKNOWN')
                mark(label,path,'%d bytes | %s' % (n,digest))
            except OSError as e: mark('UNCHECKED',path,ascii(str(e)))
    print('Copies:', ', '.join('%s=%d'%v for v in totals.items()))
    print('R2: '+('recognized for new logs (identity only).' if totals['R2'] else 'NO VERIFIED COPY. Hold new-log intake; do not substitute.'))
    print('Earlier: '+('retain only; not for new logs.' if totals['EARLIER'] else 'NO VERIFIED COPY for the earlier archive.'))
    if totals['UNKNOWN'] or totals['UNCHECKED']: print('Hold UNKNOWN and UNCHECKED separately; do not open, run or relabel as issued.')
    return 2 if totals['UNCHECKED'] else (1 if not totals['R2'] or totals['UNKNOWN'] else 0)

if __name__ == '__main__':
    if len(sys.argv)!=2 or not sys.argv[1]: sys.exit('Usage: python3 -I identify.py "folder of copies"')
    sys.exit(scan(Path(sys.argv[1])))
