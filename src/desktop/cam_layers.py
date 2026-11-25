"""Read numbered CAM inputs, emit plotted geometry and bounds in millimetres.

An X2 file polarity describes intended manufacture. Show the *plotted image*
(e.g. mask openings), respecting local clear exposures within that layer.
"""
from collections import Counter
from importlib.metadata import version
import json
import math
from pathlib import Path
import sys
import warnings


def main():
    try:
        from gerbonara import ExcellonFile, GerberFile
        from gerbonara.utils import MM
    except ImportError as exc:
        raise ValueError(f'Install Gerbonara for Studio’s python3 to plot CAM layers ({exc}).') from exc

    rows, boxes = [], []
    for i, item in enumerate(json.loads(Path('inputs.json').read_text(encoding='utf-8'))):
        try:
            # Do not pass real filenames, enable includes, scan for external
            # tool tables, or let a layer-stack loader guess the transformations.
            cls = {'Gerber': GerberFile, 'Excellon': ExcellonFile}[item['format']]
            with warnings.catch_warnings(record=True) as messages:
                warnings.simplefilter('always')
                plot = cls.from_string(Path(f'{i}.cam').read_text(encoding='utf-8-sig'))
                if plot.import_settings.unit is None or any(obj.unit is None for obj in plot.objects):
                    raise ValueError('The file does not specify units for its geometry.')
                box = plot.bounding_box(MM)
                with Path(f'{i}.geometry').open('w', encoding='utf-8') as out:
                    for obj in plot.svg_objects(svg_unit=MM, fg='white', bg='black'):
                        print(obj, file=out)
                hits = Counter(obj.tool.equivalent_width(MM) for obj in plot.drills()) if cls is ExcellonFile else {}
            notes = list(dict.fromkeys(str(w.message) for w in messages))
            for note in notes:
                # G90 specifies absolute positioning. Keep the diagnostic for its
                # location after the header. Don't pass a skipped/guessed command.
                if cls is not ExcellonFile or not note.endswith('"G90": G90 header statement found after end of header'):
                    raise ValueError(note)
            if box is not None:
                (x0, y0), (x1, y1) = box
                if not (all(map(math.isfinite, (x0, y0, x1, y1))) and x0 <= x1 and y0 <= y1):
                    raise ValueError('Invalid artwork bounds.')
                boxes.append(box)
            attrs = getattr(plot, 'file_attrs', {})
            rows.append(dict(path=item['path'], function=', '.join(attrs.get('.FileFunction', ())),
                             polarity=', '.join(attrs.get('.FilePolarity', ())), bounds=box,
                             objects=len(plot.objects), round_hits=sorted(hits.items()), warnings=notes))
        except Exception as exc:
            raise ValueError(f"{item['path']}: {exc}") from exc

    if not boxes:
        raise ValueError('No plotted geometry in the selected files.')
    bounds = [[op(b[k][j] for b in boxes) for j in range(2)] for k, op in enumerate((min, max))]
    Path('plots.json').write_text(json.dumps(dict(renderer='Gerbonara ' + version('gerbonara'),
        bounds=bounds, layers=rows), allow_nan=False), encoding='utf-8')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        sys.exit(f'CAM overlay unavailable: {error}')
