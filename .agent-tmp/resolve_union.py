import sys

CONFLICT_START = '<<<<<<< '
CONFLICT_MID = '======='
CONFLICT_END = '>>>>>>> '

def resolve_union(path):
    """Take both sides of every conflict (ours first, then theirs)."""
    lines = open(path, encoding='utf-8').read().split('\n')
    out = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith(CONFLICT_START):
            ours = []
            theirs = []
            i += 1
            while i < len(lines) and lines[i] != CONFLICT_MID:
                ours.append(lines[i])
                i += 1
            i += 1  # skip =======
            while i < len(lines) and not lines[i].startswith(CONFLICT_END):
                theirs.append(lines[i])
                i += 1
            i += 1  # skip >>>>>>>
            out.extend(ours)
            out.extend(theirs)
            continue
        out.append(line)
        i += 1
    open(path, 'w', encoding='utf-8', newline='\n').write('\n'.join(out))
    print(f'union: {path}')

if __name__ == '__main__':
    for p in sys.argv[1:]:
        resolve_union(p)
