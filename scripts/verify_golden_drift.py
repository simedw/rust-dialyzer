#!/usr/bin/env python3
import hashlib,json,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'.cache/golden_drift';OUT.mkdir(parents=True,exist_ok=True)
suite=ROOT/'.cache/upstream/otp/lib/dialyzer/test/opaque_SUITE_data'
inputs=[*sorted((suite/'src/ewgi').glob('*')),suite/'results/ewgi',suite/'dialyzer_options',ROOT/'.cache/upstream/otp/lib/dialyzer/test/dialyzer_common.erl',ROOT/'.cache/upstream/otp/lib/dialyzer/test/file_utils.erl',ROOT/'.cache/corpus/otp/base.plt']
source=[ROOT/'scripts/golden_drift_oracle.exs',Path(__file__).resolve()]
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def hashes(ps):return {str(p):digest(p)for p in ps}
before=hashes(inputs);implementation=hashes(source)
result=json.loads(subprocess.check_output(['elixir',str(ROOT/'scripts/golden_drift_oracle.exs'),str(ROOT)],cwd=ROOT))
assert result['original_golden']=='differ' and result['single_header_line_removed']=='same' and result['differing_warning_lines']==3
assert before==hashes(inputs) and implementation==hashes(source),'pinned input or repro changed'
result.update(case='otp/opaque/ewgi',complete_causal_source_line_reproduction=True,original_reference_matches=False,semantic_oracle_modified=False,inputs_sha256=before,implementation_sha256=implementation)
(OUT/'report.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:result[k]for k in ['case','original_golden','single_header_line_removed','differing_warning_lines','semantic_oracle_modified']}))
