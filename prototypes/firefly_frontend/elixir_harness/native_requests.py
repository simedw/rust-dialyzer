"""Whitelist compiled BEAM paths, analysis options and frozen runtime context."""

CONTEXT_FIELDS = ('version', 'profile', 'atom_layout', 'contract_closure_profile')
OPTION_FIELDS = ('analysis_mode', 'legal_warnings', 'purged_modules', 'use_contracts', 'solvers', 'location')

def request(row, *, emit_core=False):
    result = {key: row[key] for key in OPTION_FIELDS if key in row}
    result.update(id=row['id'], beam_paths=row['beam_paths'],
                  context={key: row['core'][key] for key in CONTEXT_FIELDS}, emit_core=emit_core)
    assert 'modules' not in result['context'] and 'core' not in result and 'warnings' not in result
    return result
