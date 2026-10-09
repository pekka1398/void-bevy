"""Thirty-day accuracy study; separate from the game, bounded reference memory."""
import json
import math
import statistics
import sys
import time
import rebound

reference = json.load(open(sys.argv[1]))
other = json.load(open(sys.argv[2]))
assert reference['end_time'] == other['end_time']
convergence = max(math.dist(a, b) for a, b in zip(reference['final_q'], other['final_q']))
print(json.dumps(dict(reference_convergence_max_position_m=convergence, end_time=reference['end_time'])), flush=True)
for method, epsilon, factor in [('ias15', 1e-9, 1), ('ias15', 1e-12, 1), ('whfast', None, 0.0625)]:
    times = []
    row = None
    for repeat in range(4):
        sim = rebound.Simulation()
        sim.G = 1
        for gm, q, v in zip(reference['gm'], reference['q'], reference['v']):
            sim.add(m=gm, x=q[0], y=q[1], z=q[2], vx=v[0], vy=v[1], vz=v[2])
        sim.integrator = method
        sim.dt = reference['base_h'] * factor
        if epsilon is not None:
            sim.integrator.epsilon = epsilon
        else:
            sim.integrator.safe_mode = 0
        e0 = sim.energy()
        start = time.perf_counter()
        stop = reference['end_time'] if epsilon is not None else reference['end_time'] - sim.dt*0.5
        sim.integrate(stop, exact_finish_time=1 if epsilon is not None else 0)
        sim.synchronize()
        elapsed = time.perf_counter() - start
        assert abs(sim.t-reference['end_time']) < 1e-3
        if repeat:
            times.append(elapsed)
        ps = [(p.x, p.y, p.z) for p in sim.particles]
        vs = [(p.vx, p.vy, p.vz) for p in sim.particles]
        maxp = max(math.dist(a,b) for a,b in zip(ps,reference['final_q']))
        maxv = max(math.dist(a,b) for a,b in zip(vs,reference['final_v']))
        maxrelative = 0
        worst = None
        for i, parent in enumerate(reference['parents']):
            if parent is None:
                continue
            got = [ps[i][c]-ps[parent][c] for c in range(3)]
            want = [reference['final_q'][i][c]-reference['final_q'][parent][c] for c in range(3)]
            error = math.dist(got,want)
            if error > maxrelative:
                maxrelative, worst = error, reference['ids'][i]
        row = dict(method=method, epsilon=epsilon, factor=factor, max_position_m=maxp,
                   max_velocity_m_s=maxv, max_parent_relative_m=maxrelative, worst_body=worst,
                   relative_energy_drift=(sim.energy()-e0)/e0, final_time_error_seconds=sim.t-reference['end_time'])
    row.update(median_seconds=statistics.median(times), seconds=times, rebound_version=rebound.__version__)
    print(json.dumps(row), flush=True)
