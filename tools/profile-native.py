#!/usr/bin/env python3
"""Capture actual native CPU stacks with perf. This tool never opens a viewer.
Build the target first, then pass the executable and its arguments after --.
"""
import argparse
import pathlib
import shlex
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', default='lab-log/native-perf.data')
    parser.add_argument('--stacks', help='also write perf script stack samples to this file')
    parser.add_argument('--frequency', type=int, default=99)
    parser.add_argument('--dry-run', action='store_true')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command and command[0] == '--':
        command = command[1:]
    if not command:
        parser.error('supply an executable and arguments after --')
    if args.frequency <= 0:
        parser.error('frequency must be positive')
    perf = shutil.which('perf')
    if perf is None:
        parser.error('perf is not installed')
    target = pathlib.Path(command[0])
    if not target.is_file():
        parser.error(f'executable does not exist: {target}; build it first')
    command[0] = str(target.resolve())
    output = pathlib.Path(args.output)
    capture = [perf, 'record', '--event', 'cpu-clock:u', '--freq', str(args.frequency),
               '--call-graph', 'dwarf', '--output', str(output), '--', *command]
    print(shlex.join(capture), flush=True)
    if args.dry_run:
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    # Permission or sampling failures are fatal; do not substitute fake samples or another event.
    subprocess.run(capture, check=True)
    if args.stacks:
        stacks = pathlib.Path(args.stacks)
        stacks.parent.mkdir(parents=True, exist_ok=True)
        with stacks.open('wb') as stream:
            subprocess.run([perf, 'script', '--input', str(output)], stdout=stream, check=True)


if __name__ == '__main__':
    main()
