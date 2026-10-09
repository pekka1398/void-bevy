"""Fail closed before starting a diagnostic command if containment is missing."""
import json
import os
from pathlib import Path
import sys

entry = next(line for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))
group = Path('/sys/fs/cgroup') / entry[3:].lstrip('/')
expected = {
    'memory.high': str(2 * 1024**3),
    'memory.max': str(3 * 1024**3),
    'memory.swap.max': '0',
    'cpu.max': '200000 100000',
    'pids.max': '96',
}
actual = {key: (group / key).read_text().strip() for key in expected}
if actual != expected or Path('/dev/dri').exists() or Path('/dev/nvidia0').exists():
    raise SystemExit(f'Refusing uncontained diagnostic: {actual}')
print('Verified containment: ' + json.dumps(actual), flush=True)
if len(sys.argv) < 2:
    raise SystemExit('Missing diagnostic command')
os.execvp(sys.argv[1], sys.argv[1:])
