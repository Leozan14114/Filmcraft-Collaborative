# Executable model of the collaboration protocol (merge3 + sequencer + client rebase), used to
# check convergence before the Rust was written. Run: python3 docs/collab-protocol-model.py
import random, copy, json

def merge3(base, mine, theirs):
    """returns (value, conflicts). value None = deleted."""
    if mine == theirs: return mine, 0
    if mine == base: return theirs, 0
    if theirs == base: return mine, 0
    if isinstance(base, dict) and isinstance(mine, dict) and isinstance(theirs, dict):
        out, c = {}, 0
        for k in list(dict.fromkeys(list(theirs) + list(mine) + list(base))):
            b, m, t = base.get(k), mine.get(k), theirs.get(k)
            v, cc = merge3(b, m, t); c += cc
            if v is not None or (k in mine and mine[k] is None) : out[k] = v
        return out, c
    if all(isinstance(x, list) for x in (base, mine, theirs)) and all(
        isinstance(e, dict) and 'id' in e for l in (base, mine, theirs) for e in l):
        bm = {e['id']: e for e in base}; mm = {e['id']: e for e in mine}; tm = {e['id']: e for e in theirs}
        c = 0; res = []
        for e in theirs:
            i = e['id']
            if i in bm and i not in mm:  # I deleted
                if e == bm[i]: continue
                c += 1; res.append(e); continue
            if i in bm:
                v, cc = merge3(bm[i], mm[i], e); c += cc; res.append(v)
            else: res.append(e)
        ids = {e['id'] for e in res}
        for idx, e in enumerate(mine):
            i = e['id']
            if i in bm or i in ids: continue
            pos = 0
            for j in range(idx - 1, -1, -1):
                pid = mine[j]['id']
                if pid in ids:
                    pos = [x['id'] for x in res].index(pid) + 1; break
            res.insert(pos, e); ids.add(i)
        # their deletions of things I modified: keep mine
        for e in mine:
            i = e['id']
            if i in bm and i not in tm and e != bm[i] and i not in ids:
                res.append(e); ids.add(i); c += 1
        return res, c
    return theirs, 1   # atomic conflict: theirs wins

class Server:
    def __init__(s): s.rev = 0; s.state = {}; s.krev = {}; s.out = {}
    def op(s, cid, opid, base, changes):
        for k in changes:
            if s.krev.get(k, 0) > base:
                return ('rej', opid)
        s.rev += 1
        for k, v in changes.items():
            if v is None: s.state.pop(k, None)
            else: s.state[k] = v
            s.krev[k] = s.rev
        return ('ok', s.rev, cid, opid, changes)

class Client:
    def __init__(s, cid): s.id = cid; s.mirror = {}; s.rev = 0; s.local = {}; s.inflight = None; s.n = 0; s.conf = 0
    def dirty(s): return [k for k in set(s.local) | set(s.mirror) if s.local.get(k) != s.mirror.get(k)]
    def try_send(s):
        if s.inflight or not s.dirty(): return None
        s.n += 1
        ch = {k: s.local.get(k) for k in s.dirty()}
        s.inflight = (s.n, ch)
        return (s.id, s.n, s.rev, ch)
    def on(s, msg):
        if msg[0] == 'ok':
            _, rev, cid, opid, ch = msg
            assert rev == s.rev + 1, "gap"
            if cid == s.id and s.inflight and s.inflight[0] == opid:
                s.inflight = None
                for k, v in ch.items():
                    if v is None: s.mirror.pop(k, None)
                    else: s.mirror[k] = v
            else:
                for k, t in ch.items():
                    b, m = s.mirror.get(k), s.local.get(k)
                    v, c = merge3(b, m, t); s.conf += c
                    if v is None: s.local.pop(k, None)
                    else: s.local[k] = v
                    if t is None: s.mirror.pop(k, None)
                    else: s.mirror[k] = t
            s.rev = rev
        elif msg[0] == 'rej':
            s.inflight = None

def run(seed):
    r = random.Random(seed)
    srv = Server(); cl = [Client(i) for i in range(3)]
    queues = [[] for _ in cl]   # server->client ordered streams
    up = [[] for _ in cl]       # client->server ordered streams
    # seed: client0 creates doc
    for step in range(400):
        c = r.choice(range(3))
        a = r.random()
        if a < 0.35:   # local edit
            cc = cl[c]; key = r.choice(['t1', 't2', 'meta'])
            cur = cc.local.get(key, {'id': key, 'clips': []})
            cur = copy.deepcopy(cur)
            x = r.random()
            if x < 0.5 or not cur['clips']:
                cur['clips'].append({'id': r.randint(1, 10**9), 'start': r.randint(0, 99)})
            elif x < 0.8:
                r.choice(cur['clips'])['start'] = r.randint(0, 99)
            else:
                cur['clips'].pop(r.randrange(len(cur['clips'])))
            cc.local[key] = cur
        elif a < 0.6:  # client sends
            m = cl[c].try_send()
            if m: up[c].append(m)
        elif a < 0.8:  # server processes one upstream message
            ci = r.choice(range(3))
            if up[ci]:
                cid, opid, base, ch = up[ci].pop(0)
                res = srv.op(cid, opid, base, ch)
                if res[0] == 'ok':
                    for q in queues: q.append(res)
                else:
                    queues[cid].append(res)
        else:          # deliver
            if queues[c]: cl[c].on(queues[c].pop(0))
    # drain
    for _ in range(5000):
        prog = False
        for i in range(3):
            if queues[i]: cl[i].on(queues[i].pop(0)); prog = True
            m = cl[i].try_send()
            if m: up[i].append(m); prog = True
            if up[i]:
                cid, opid, base, ch = up[i].pop(0); res = srv.op(cid, opid, base, ch); prog = True
                if res[0] == 'ok':
                    for q in queues: q.append(res)
                else: queues[cid].append(res)
        if not prog: break
    for c in cl:
        assert c.local == srv.state, (seed, 'local!=server')
        assert c.mirror == srv.state
    return sum(c.conf for c in cl)

tot = 0
for s in range(300): tot += run(s)
print("300 seeds converged; total leaf conflicts reported:", tot)
