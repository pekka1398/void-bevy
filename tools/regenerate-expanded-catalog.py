#!/usr/bin/env python3
"""Regenerate authored gameplay initial conditions from checked-in source snapshots only."""
import json
import math
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
SYSTEMS = ROOT / 'crates/orbit/systems'
G = 6.6743e-11
AU = 149597870700
source = json.loads((SYSTEMS / 'sources/expanded-catalog.json').read_text())
spec = json.loads((SYSTEMS / 'sol.json').read_text())
spec['name'] = 'Sol analogue · expanded exploration catalog v1'
spec['root']['children'].append(source['authored']['pluto']['initial_body_spec'])

def spin(period):
    return dict(periodSeconds=period, obliquityRadians=0, poleLongitudeRadians=0, angleAtEpochRadians=0)

def body(name, mass, radius, a, e, i, node, w, ma, period, plane):
    return dict(id=name.lower().replace('-', ''), name=name, massKg=mass, radiusMeters=radius,
                color='#a6a09b', rotation=spin(period), orbit=dict(semiMajorAxisMeters=a,
                eccentricity=e, inclinationRadians=math.radians(i),
                longitudeOfAscendingNodeRadians=math.radians(node),
                argumentOfPeriapsisRadians=math.radians(w), meanAnomalyRadians=math.radians(ma)),
                orbitPlane=plane, children=[])

parents = dict(Mars='ares', Jupiter='velvet', Saturn='halo', Uranus='azure', Neptune='abyss', Pluto='pluto')
for id, entry in source['satellites'].items():
    row = entry['elements_row']
    physical = entry['physical_row']
    radius = float(physical[6].split()[0]) * 1000
    if entry['mass_method'].startswith('authored'):
        mass = 4 / 3 * math.pi * radius**3 * 1000
    else:
        mass = float(physical[3].split()[0]) * 1e9 / G
    b = body(row[2], mass, radius, float(row[7])*1000, float(row[8]), float(row[11]),
             float(row[12]), float(row[9]), float(row[10]), float(row[13])*86400,
             'ecliptic' if row[5] == 'ecliptic' else 'parent-equator')
    if entry['spin_method'].startswith('authored synchronous'):
        b['rotation'] = dict(kind='locked', periodSeconds=float(row[13])*86400, obliquityToOrbitRadians=0)
    else:
        b['rotation'] = spin(86400)
    next(p for p in spec['root']['children'] if p['id'] == parents[row[1]])['children'].append(b)

aliases = dict(ember='Io', rime='Europa', hollow='Ganymede', umber='Callisto', haze='Titan', selene='Moon')
for parent in spec['root']['children']:
    for b in parent['children']:
        if b['id'] in aliases:
            b['name'] += ' (' + aliases[b['id']] + ')'

names = dict(ceres='Ceres', vesta='Vesta', pallas='Pallas', hygiea='Hygiea', eros='Eros', bennu='Bennu',
             ryugu='Ryugu', eris='Eris', haumea='Haumea', makemake='Makemake', quaoar='Quaoar',
             orcus='Orcus', gonggong='Gonggong', sedna='Sedna', halley='Halley', **{'67p': '67P'},
             encke='Encke', halebopp='Hale-Bopp')
for id, entry in source['small_bodies'].items():
    j = entry['response']
    el = {v['name']: float(v['value']) for v in j['orbit']['elements']}
    pp = {v['name']: v for v in j.get('phys_par', [])}
    if 'physical_override' in entry:
        row = entry['physical_override']['row']
        radius = float(row[2].split()[0])*1000
        mass = float(row[3].split()[0])*1e18
        period = float(row[5].split()[0])*86400
    else:
        diameter = float(pp['diameter']['value']) if 'diameter' in pp else float(re.search(r'estimate ([\d.]+)', entry['diameter_method'])[1])
        radius = diameter*500
        if 'GM' in pp:
            mass = float(pp['GM']['value'])*1e9/G
        else:
            density = float(re.search(r'estimate ([\d.]+)', entry['mass_method'])[1])
            mass = 4/3*math.pi*radius**3*density
        period = float(pp['rot_per']['value'])*3600 if 'rot_per' in pp else 86400
    spec['root']['children'].append(body(names[id], mass, radius, el['a']*AU, el['e'],
        el['i'], el['om'], el['w'], el['ma'], period, 'ecliptic'))
for b in spec['root']['children']:
    b['children'].sort(key=lambda child: child['orbit']['semiMajorAxisMeters'])
spec['root']['children'].sort(key=lambda child: child['orbit']['semiMajorAxisMeters'])
(SYSTEMS / 'sol-expanded.json').write_text(json.dumps(spec, indent=2)+'\n')
