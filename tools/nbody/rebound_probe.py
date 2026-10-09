"""Optional REBOUND 5.2.2 experiment; never called by the game or required tests."""
import array
import json
import math
from pathlib import Path
import statistics
import struct
import sys
import time
import rebound

raw = Path(sys.argv[1]).read_bytes()
n, steps = struct.unpack_from('<II', raw)
floats = array.array('d')
floats.frombytes(raw[8:])
if sys.byteorder != 'little':
    floats.byteswap()
h = floats[0]
base_h = h * 8
end = steps * h
assert steps % 128 == 0, 'Use fixture refinement=8, base_steps=832 for aligned WHFast steps'
gm = floats[16:16+n]
q = floats[16+n:16+4*n]
v = floats[16+4*n:16+7*n]
reference = floats[-6*n:]
initial_energy = None
for method, factor, epsilon in [('whfast', 0.25, None), ('whfast', 1, None), ('whfast', 4, None), ('whfast', 16, None), ('whfast-corrector11', 1, None), ('ias15', 1, 1e-9), ('ias15', 1, 1e-12)]:
    times = []
    row = None
    for repeat in range(4):
        sim = rebound.Simulation()
        sim.G = 1
        for i in range(n):
            sim.add(m=gm[i], x=q[3*i], y=q[3*i+1], z=q[3*i+2], vx=v[3*i], vy=v[3*i+1], vz=v[3*i+2])
        sim.integrator = 'whfast' if method.startswith('whfast') else 'ias15'
        sim.dt = base_h * factor
        if method.startswith('whfast'):
            sim.integrator.safe_mode = 0
            if 'corrector' in method:
                sim.integrator.corrector = 11
        else:
            sim.integrator.epsilon = epsilon
        energy0 = sim.energy()
        start = time.perf_counter()
        # Stop at the common full step, avoiding an extra step from accumulated clock rounding.
        stop = end - 0.5*sim.dt if method.startswith('whfast') else end
        sim.integrate(stop, exact_finish_time=0 if method.startswith('whfast') else 1)
        sim.synchronize()
        elapsed = time.perf_counter() - start
        assert abs(sim.t-end) < 1e-5, (method, sim.t, end)
        if repeat:
            times.append(elapsed)
        position_error = velocity_error = 0
        for i, p in enumerate(sim.particles):
            position_error = max(position_error, math.dist((p.x, p.y, p.z), reference[6*i:6*i+3]))
            velocity_error = max(velocity_error, math.dist((p.vx, p.vy, p.vz), reference[6*i+3:6*i+6]))
        row = dict(method=method, step_factor=factor, epsilon=epsilon, max_position_m=position_error,
                   max_velocity_m_s=velocity_error, relative_energy_drift=(sim.energy()-energy0)/energy0,
                   final_time_error_seconds=sim.t-end)
    row.update(rebound_version=rebound.__version__, median_seconds=statistics.median(times), seconds=times,
               reference='scalar Yoshida8 h/8, raw states', bodies=n, duration_seconds=end)
    print(json.dumps(row), flush=True)
