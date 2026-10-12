#!/usr/bin/env python3
"""Create a compiler-free frontend manifest from a BEAM inventory."""
import argparse,json
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('beam_list',type=Path);p.add_argument('output',type=Path)
p.add_argument('--dialyzer-ebin',type=Path,required=True)
a=p.parse_args()
beams=[str(Path(line).resolve()) for line in a.beam_list.read_text().splitlines() if line.strip()]
assert beams and all(Path(beam).is_file() for beam in beams)
manifest={'beam_files':beams}
for name in ['typesig','contracts']:
 path=(a.dialyzer_ebin/f'dialyzer_{name}.beam').resolve()
 assert path.is_file(),path
 manifest[f'dialyzer_{name}_beam']=str(path)
a.output.write_text(json.dumps(manifest,indent=2)+'\n')
