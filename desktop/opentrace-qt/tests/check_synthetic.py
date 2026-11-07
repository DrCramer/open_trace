# Self-test check: the genotypes called by the native app for the invented run files of the core equal the genotypes
# the files were made with (fsa/expected.json written by the self-test). Usage: python3 check_synthetic.py <self-test out dir>
import json, sys
out = sys.argv[1]
exp = json.load(open(out + '/fsa/expected.json'))
got = {}
for line in open(out + '/digest.txt', encoding='utf-8').read().splitlines():
    f, _, calls = line.partition(':')
    m = {}
    for c in filter(None, calls.split(',')):
        k, _, v = c.partition('=')
        m.setdefault(k, []).append(v)
    got[f] = m
bad = 0
for f, markers in exp.items():
    for mk, labels in markers.items():
        if sorted(got.get(f, {}).get(mk, [])) != sorted(labels):
            bad += 1
            if bad < 10: print('MISMATCH', f, mk, labels, got.get(f, {}).get(mk))
n = sum(len(m) for m in exp.values())
print(f'{n - bad}/{n} markers as expected')
sys.exit(1 if bad else 0)
