#!/usr/bin/env python3
"""Check an STL for the defects that make Bambu Studio offer Repair or print a floating piece.

Exit 0 only when every file is one closed solid: each edge shared by two
oppositely wound faces, no degenerate triangles, no second solid, and no
island that starts in mid-air. A void inside that solid is allowed.
"""

import math
import struct
import sys
from collections import defaultdict


LAYER_H = 0.2
GRID = 0.4
WELD = 1e-4
BED = 0.05


def load_stl(path):
    data = open(path, "rb").read()
    if len(data) >= 84:
        count = struct.unpack_from("<I", data, 80)[0]
        if 84 + count * 50 == len(data):
            tris = []
            off = 84
            for _ in range(count):
                nums = struct.unpack_from("<12fH", data, off)
                tris.append((nums[3:6], nums[6:9], nums[9:12]))
                off += 50
            return tris
    text = data.decode("utf-8", errors="replace")
    tris = []
    verts = []
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 4 and parts[0] == "vertex":
            verts.append(tuple(float(p) for p in parts[1:4]))
            if len(verts) == 3:
                tris.append(tuple(verts))
                verts = []
    return tris


def weld(tris):
    index = {}
    verts = []
    faces = []

    def vid(p):
        key = tuple(int(round(c / WELD)) for c in p)
        if key not in index:
            index[key] = len(verts)
            verts.append(p)
        return index[key]

    for a, b, c in tris:
        faces.append((vid(a), vid(b), vid(c)))
    return verts, faces


def area2(verts, face):
    ax, ay, az = verts[face[0]]
    bx, by, bz = verts[face[1]]
    cx, cy, cz = verts[face[2]]
    ux, uy, uz = bx - ax, by - ay, bz - az
    vx, vy, vz = cx - ax, cy - ay, cz - az
    cross = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    return cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]


def signed_volume(verts, faces):
    volume = 0.0
    for i, j, k in faces:
        ax, ay, az = verts[i]
        bx, by, bz = verts[j]
        cx, cy, cz = verts[k]
        volume += ax * (by * cz - bz * cy) - ay * (bx * cz - bz * cx) + az * (bx * cy - by * cx)
    return volume / 6.0


def components(faces):
    edge_faces = defaultdict(list)
    for fi, (a, b, c) in enumerate(faces):
        for u, v in ((a, b), (b, c), (c, a)):
            edge_faces[(u, v) if u < v else (v, u)].append(fi)
    parent = list(range(len(faces)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    def union(i, j):
        ri, rj = find(i), find(j)
        if ri != rj:
            parent[rj] = ri

    open_edges = 0
    nonmanifold = 0
    inconsistent = 0
    for (u, v), users in edge_faces.items():
        if len(users) == 1:
            open_edges += 1
        elif len(users) > 2:
            nonmanifold += 1
        else:
            union(users[0], users[1])
            fa = faces[users[0]]
            fb = faces[users[1]]
            a_dir = _directed(fa, u, v)
            b_dir = _directed(fb, u, v)
            if a_dir == b_dir:
                inconsistent += 1
    groups = defaultdict(list)
    for fi in range(len(faces)):
        groups[find(fi)].append(fi)
    return open_edges, nonmanifold, inconsistent, list(groups.values())


def _directed(face, u, v):
    a, b, c = face
    cycle = ((a, b), (b, c), (c, a))
    if (u, v) in cycle:
        return 1
    if (v, u) in cycle:
        return -1
    return 0


def ray_hits(origin, verts, faces):
    ox, oy, oz = origin
    dx, dy, dz = 1.0, 0.013, 0.017
    hits = 0
    for i, j, k in faces:
        ax, ay, az = verts[i]
        bx, by, bz = verts[j]
        cx, cy, cz = verts[k]
        abx, aby, abz = bx - ax, by - ay, bz - az
        acx, acy, acz = cx - ax, cy - ay, cz - az
        px, py, pz = dy * acz - dz * acy, dz * acx - dx * acz, dx * acy - dy * acx
        det = abx * px + aby * py + abz * pz
        if abs(det) < 1e-12:
            continue
        inv = 1.0 / det
        tx, ty, tz = ox - ax, oy - ay, oz - az
        u = (tx * px + ty * py + tz * pz) * inv
        if u < 0.0 or u > 1.0:
            continue
        qx, qy, qz = ty * abz - tz * aby, tz * abx - tx * abz, tx * aby - ty * abx
        v = (dx * qx + dy * qy + dz * qz) * inv
        if v < 0.0 or u + v > 1.0:
            continue
        t = (acx * qx + acy * qy + acz * qz) * inv
        if t > 1e-8:
            hits += 1
    return hits


def centroid(verts, face):
    xs = [verts[i][0] for i in face]
    ys = [verts[i][1] for i in face]
    zs = [verts[i][2] for i in face]
    return (sum(xs) / 3.0, sum(ys) / 3.0, sum(zs) / 3.0)


def classify_shells(verts, faces, groups):
    shells = []
    for group in groups:
        shell_faces = [faces[i] for i in group]
        shells.append((signed_volume(verts, shell_faces), shell_faces))
    shells.sort(key=lambda item: abs(item[0]), reverse=True)
    body_faces = shells[0][1]
    body_sign = 1.0 if shells[0][0] >= 0 else -1.0
    extra = []
    cavities = 0
    for volume, shell_faces in shells[1:]:
        point = centroid(verts, shell_faces[0])
        inside = ray_hits(point, verts, body_faces) % 2 == 1
        if inside and volume * body_sign < 0:
            cavities += 1
        else:
            extra.append(point)
    return cavities, extra


def plane_segments(verts, faces, z):
    segments = []
    for a, b, c in faces:
        pts = []
        for u, v in ((a, b), (b, c), (c, a)):
            z0 = verts[u][2]
            z1 = verts[v][2]
            lo, hi = (z0, z1) if z0 <= z1 else (z1, z0)
            if lo < z <= hi and z0 != z1:
                t = (z - z0) / (z1 - z0)
                p0 = verts[u]
                p1 = verts[v]
                pts.append((p0[0] + t * (p1[0] - p0[0]), p0[1] + t * (p1[1] - p0[1])))
        if len(pts) >= 2:
            segments.append((pts[0], pts[1]))
    return segments


def filled_cells(segments):
    rows = defaultdict(list)
    for (x0, y0), (x1, y1) in segments:
        if y0 == y1:
            continue
        y_lo, y_hi = (y0, y1) if y0 < y1 else (y1, y0)
        j0 = math.floor(y_lo / GRID)
        j1 = math.floor(y_hi / GRID)
        for j in range(j0, j1 + 1):
            y = (j + 0.5) * GRID
            if y_lo < y <= y_hi:
                t = (y - y0) / (y1 - y0)
                rows[j].append(x0 + t * (x1 - x0))
    cells = set()
    for j, xs in rows.items():
        xs.sort()
        for left, right in zip(xs[0::2], xs[1::2]):
            i0 = math.floor(left / GRID)
            i1 = math.floor(right / GRID)
            for i in range(i0, i1 + 1):
                cells.add((i, j))
    return cells


def dilate(cells):
    out = set(cells)
    for x, y in cells:
        out.add((x + 1, y))
        out.add((x - 1, y))
        out.add((x, y + 1))
        out.add((x, y - 1))
    return out


def floating_islands(verts, faces):
    zs = [p[2] for p in verts]
    if not zs:
        return ["empty mesh"]
    z0 = min(zs)
    z1 = max(zs)
    previous = set()
    islands = []
    z = z0 + LAYER_H * 0.5
    steps = 0
    first = True
    while z < z1 and steps < 4000:
        steps += 1
        cells = filled_cells(plane_segments(verts, faces, z))
        if cells and not first:
            seen = set()
            supported = dilate(previous)
            for start in cells:
                if start in seen:
                    continue
                stack = [start]
                seen.add(start)
                touches = False
                while stack:
                    current = stack.pop()
                    if current in supported:
                        touches = True
                    x, y = current
                    for nxt in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                        if nxt in cells and nxt not in seen:
                            seen.add(nxt)
                            stack.append(nxt)
                if not touches:
                    islands.append(round(z - z0, 3))
                    if len(islands) >= 8:
                        return islands
        previous = cells
        first = False
        z += LAYER_H
    return islands


def bounds(verts):
    xs = [p[0] for p in verts]
    ys = [p[1] for p in verts]
    zs = [p[2] for p in verts]
    return min(xs), min(ys), min(zs), max(xs), max(ys), max(zs)


def check(path):
    problems = []
    try:
        raw = load_stl(path)
    except OSError as err:
        return [str(err)]
    if not raw:
        return ["no triangles"]
    verts, faces = weld(raw)
    degenerate = sum(1 for face in faces if len(set(face)) < 3 or area2(verts, face) < 1e-24)
    faces = [face for face in faces if len(set(face)) == 3 and area2(verts, face) >= 1e-24]
    if degenerate:
        problems.append(f"degenerate triangles: {degenerate}")
    if not faces:
        problems.append("no triangles left after dropping degenerates")
        return problems
    open_edges, nonmanifold, inconsistent, groups = components(faces)
    if open_edges:
        problems.append(f"open edges: {open_edges}")
    if nonmanifold:
        problems.append(f"non-manifold edges: {nonmanifold}")
    if inconsistent:
        problems.append(f"inconsistent winding: {inconsistent}")
    if open_edges or nonmanifold or inconsistent or degenerate:
        problems.append("Bambu Studio will offer Repair")
    else:
        cavities, extra = classify_shells(verts, faces, groups)
        if extra:
            problems.append(f"extra solids: {len(extra)} (floating or disconnected)")
        elif len(groups) != 1 + cavities:
            problems.append(f"shells: {len(groups)}")
    islands = floating_islands(verts, faces)
    if islands:
        shown = ", ".join(str(z) for z in islands)
        problems.append(f"floating islands at {shown} mm above the lowest point")
    x0, y0, z0, x1, y1, z1 = bounds(verts)
    size = f"{x1 - x0:.2f} x {y1 - y0:.2f} x {z1 - z0:.2f} mm"
    bed = "bed z=0" if abs(z0) <= BED else f"lowest z={z0:.3f} (drop onto the plate before printing)"
    return problems, size, len(faces), bed


def main(argv):
    if len(argv) < 2:
        print("usage: check_stl.py part.stl [more.stl ...]", file=sys.stderr)
        return 2
    failed = 0
    for path in argv[1:]:
        result = check(path)
        if isinstance(result, list):
            print(f"FAIL {path}")
            for item in result:
                print(f"  {item}")
            failed += 1
            continue
        problems, size, count, bed = result
        status = "FAIL" if problems else "ok"
        print(f"{status} {path}")
        print(f"  size {size}, triangles {count}, {bed}")
        for item in problems:
            print(f"  {item}")
        if problems:
            failed += 1
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
