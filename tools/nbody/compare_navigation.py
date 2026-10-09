"""Serial release navigation comparison; build examples before running this script.

Run from repository root. Logs include source revision/diff digest and full solutions.
Wall times are headless solver measurements, not frame times.
"""
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    output = Path('lab-log/nbody/navigation-comparison')
    output.mkdir(parents=True, exist_ok=True)
    binary = Path('target/release/examples/navigation_profile')
    diff = subprocess.check_output(['git', 'diff', 'HEAD', '--', 'crates/orbit'])
    manifest = {
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
        'tracked_diff_sha256': hashlib.sha256(diff).hexdigest(),
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'source_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in sorted(Path('crates/orbit').rglob('*.rs'))},
        'note': 'Compile before running; hashes identify tracked and untracked Rust sources.',
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2))
    oracle = None
    summary = []
    # Reverse ordering on the second pass exposes drift; no concurrent workloads.
    for i, mode in enumerate(['scalar', 'auto', 'spin2', 'spin4', 'spin8',
                              'spin8', 'spin4', 'spin2', 'auto', 'scalar']):
        result = subprocess.run([str(binary), mode], capture_output=True,
                                text=True, check=True, timeout=90)
        data = json.loads(result.stdout)
        (output / f'{i:02}-{mode}.json').write_text(result.stdout)
        assert data['result'].startswith('Ok('), data['result']
        if oracle is None:
            oracle = data['result']
        assert data['result'] == oracle, f'{mode}: navigation solution changed'
        row = {k: data[k] for k in ('seconds', 'retained_bytes')}
        row.update(mode=mode, run=i)
        summary.append(row)
        print(json.dumps(row), flush=True)
    (output / 'summary.json').write_text(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
