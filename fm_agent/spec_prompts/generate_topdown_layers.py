#!/usr/bin/env python3
"""
Per-phase top-down layer computation for the FM-Agent pipeline.

Reads `fm_agent/phases.json` + everything under
`fm_agent/extracted_functions/`, builds a per-phase call graph by
language-aware regex matching against known function stems, and emits
`fm_agent/spec_prompts/phase_NN_topdown_layers.json` for each phase.

See `FM-Agent/workflow_spec_step1_layers.md` for the full algorithm
specification.

Usage:
    python3 fm_agent/spec_prompts/generate_topdown_layers.py
    python3 fm_agent/spec_prompts/generate_topdown_layers.py 1 3 8
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from collections import defaultdict

REPO = Path(__file__).resolve().parents[2]
PHASES_JSON = json.loads((REPO / "fm_agent" / "phases.json").read_text())
EXTRACTED = REPO / "fm_agent" / "extracted_functions"
OUT_DIR = REPO / "fm_agent" / "spec_prompts"

PROJECT = PHASES_JSON["project"]
EXTS = PHASES_JSON["file_extensions"]

PHASE_META = {p["phase"]: p["name"] for p in PHASES_JSON["phases"]}

# Rust call-site regex: ident, optional turbofish ::<...>, open paren.
CALL_RE = {
    "rs": re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:::<[^>]*>)?\s*\("),
}

RUST_KEYWORDS = {
    # control flow / declarations
    "if", "else", "for", "while", "loop", "match", "return", "break", "continue",
    "fn", "let", "const", "static", "mut", "pub", "use", "mod", "struct",
    "enum", "trait", "impl", "type", "where", "as", "in", "ref",
    "self", "Self", "super", "crate", "extern", "unsafe", "async", "await",
    "move", "dyn", "box", "true", "false",
    # common stdlib / builtins that show up as "calls"
    "Vec", "String", "Some", "None", "Ok", "Err", "Box", "Arc", "Mutex",
    "RwLock", "HashMap", "HashSet", "BTreeMap", "Option", "Result",
    "println", "print", "eprintln", "eprint", "format", "write", "writeln",
    "assert", "assert_eq", "assert_ne", "debug_assert", "dbg",
    "vec", "panic", "todo", "unimplemented", "unreachable", "include_str",
    "include_bytes", "env", "concat", "stringify", "matches",
    "into", "try_into", "from", "try_from", "to_string", "to_owned", "clone",
    "iter", "into_iter", "iter_mut", "next", "collect", "map", "filter",
    "and_then", "or_else", "unwrap", "unwrap_or", "expect", "is_some",
    "is_none", "is_ok", "is_err", "as_ref", "as_mut", "as_str", "as_bytes",
    "len", "push", "pop", "insert", "remove", "get", "contains", "split",
    "trim", "to_lowercase", "to_uppercase", "starts_with", "ends_with",
    "replace", "find", "join", "spawn", "block_on",
}


def strip_rust_comments(src: str) -> str:
    """Replace comment contents with spaces (preserves offsets)."""
    out = []
    i = 0
    n = len(src)
    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            out.append(" " * (j - i))
            i = j
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            j = n if j == -1 else j + 2
            out.append(" " * (j - i))
            i = j
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(" " * (j - i))
            i = j
            continue
        out.append(c)
        i += 1
    return "".join(out)


def fqn_from_path(p: Path) -> str:
    """fm_agent/extracted_functions/a/b/c-rs/foo.rs → a::b::c-rs::foo"""
    rel = p.relative_to(EXTRACTED)
    parts = list(rel.parts)
    parts[-1] = parts[-1].rsplit(".", 1)[0]
    return "::".join(parts)


def stem_of(fqn: str) -> str:
    return fqn.split("::")[-1]


def collect_phase_files(phase: int) -> list[Path]:
    files: list[Path] = []
    for p in PHASES_JSON["phases"]:
        if p["phase"] != phase:
            continue
        for module in p["modules"]:
            for src_rel in module["source_files"]:
                stem, ext = src_rel.rsplit(".", 1)
                fname = Path(src_rel).name
                base, ext2 = fname.rsplit(".", 1)
                ext_dir = f"{base}-{ext2}"
                parent_dir = Path(src_rel).parent
                fdir = EXTRACTED / parent_dir / ext_dir
                if fdir.exists():
                    files.extend(sorted(fdir.glob(f"*.{ext2}")))
    return files


def build_call_graph(phase_files: list[Path]):
    fqns = [fqn_from_path(p) for p in phase_files]
    fqn_set = set(fqns)
    stem_to_fqns: dict[str, list[str]] = defaultdict(list)
    for f in fqns:
        stem_to_fqns[stem_of(f)].append(f)

    callees_map: dict[str, set[str]] = {f: set() for f in fqns}
    callers_map: dict[str, set[str]] = {f: set() for f in fqns}
    file_for: dict[str, Path] = dict(zip(fqns, phase_files))

    for f in fqns:
        text = strip_rust_comments(file_for[f].read_text())
        own_stem = stem_of(f)
        for m in CALL_RE["rs"].finditer(text):
            ident = m.group(1)
            if ident in RUST_KEYWORDS:
                continue
            if ident == own_stem:
                continue  # self-recursion not a layer-edge for our purposes
            if ident not in stem_to_fqns:
                continue
            # Pick the unique FQN if there's only one with this stem in-phase;
            # otherwise add ALL (overcounting is safer than missing)
            for callee in stem_to_fqns[ident]:
                if callee == f:
                    continue
                callees_map[f].add(callee)
                callers_map[callee].add(f)
    return fqns, callees_map, callers_map


def topo_layers(fqns: list[str], callers_map: dict[str, set[str]]):
    """Kahn-style topo with SCC fallback for cycles."""
    remaining = set(fqns)
    layers: list[dict] = []
    assigned: set[str] = set()

    while remaining:
        # ready = remaining nodes whose callers are all already assigned
        ready = [
            f for f in remaining
            if (callers_map[f] & remaining) == set()
        ]
        if ready:
            layers.append({"layer": len(layers), "fqns": sorted(ready), "cycle": False})
            assigned.update(ready)
            remaining -= set(ready)
            continue

        # cycle in remaining → resolve via Tarjan SCC
        sub_callees = {
            f: {c for c in (callers_map[f] | set()) if c in remaining}  # placeholder; we'll rebuild below
            for f in remaining
        }
        # Build forward edges (callee → caller is callers_map; we want forward)
        # We have callers_map[node] = set of callers. We want edges_in[node] = callers in remaining.
        edges_in = {f: callers_map[f] & remaining for f in remaining}
        # For Tarjan we need outgoing edges. Out edges of f = nodes in remaining whose edges_in contain f.
        out = defaultdict(set)
        for v in remaining:
            for u in edges_in[v]:
                out[u].add(v)

        # Tarjan
        index = {}
        lowlink = {}
        on_stack = set()
        stack: list[str] = []
        sccs: list[list[str]] = []
        counter = [0]

        def strongconnect(v):
            index[v] = counter[0]
            lowlink[v] = counter[0]
            counter[0] += 1
            stack.append(v)
            on_stack.add(v)
            for w in out.get(v, ()):
                if w not in index:
                    strongconnect(w)
                    lowlink[v] = min(lowlink[v], lowlink[w])
                elif w in on_stack:
                    lowlink[v] = min(lowlink[v], index[w])
            if lowlink[v] == index[v]:
                comp = []
                while True:
                    w = stack.pop()
                    on_stack.remove(w)
                    comp.append(w)
                    if w == v:
                        break
                sccs.append(comp)

        sys.setrecursionlimit(10000)
        for v in list(remaining):
            if v not in index:
                strongconnect(v)

        # Topo-sort SCCs: an SCC is ready if every external caller is already assigned.
        scc_id = {}
        for i, comp in enumerate(sccs):
            for v in comp:
                scc_id[v] = i
        scc_callers = defaultdict(set)
        for v in remaining:
            for u in edges_in[v]:
                if scc_id[u] != scc_id[v]:
                    scc_callers[scc_id[v]].add(scc_id[u])
        scc_done: set[int] = set()
        progress = True
        while progress:
            progress = False
            for i, comp in enumerate(sccs):
                if i in scc_done:
                    continue
                if scc_callers[i] - scc_done:
                    continue
                # all outside-SCC callers already assigned → emit layer
                layers.append({
                    "layer": len(layers),
                    "fqns": sorted(comp),
                    "cycle": len(comp) > 1,
                })
                assigned.update(comp)
                remaining -= set(comp)
                scc_done.add(i)
                progress = True
        if not progress:
            # shouldn't happen, but break out to avoid infinite loop
            layers.append({
                "layer": len(layers),
                "fqns": sorted(remaining),
                "cycle": True,
            })
            remaining.clear()

    return layers


def emit_phase(phase_num: int):
    phase_name = PHASE_META[phase_num]
    files = collect_phase_files(phase_num)
    if not files:
        print(f"phase {phase_num}: no extracted files; skipping")
        return
    fqns, callees_map, callers_map = build_call_graph(files)
    layers = topo_layers(fqns, callers_map)
    file_for = {fqn_from_path(p): p for p in files}

    out = {
        "phase": phase_num,
        "phase_name": phase_name,
        "total_functions": len(fqns),
        "total_layers": len(layers),
        "layers": [],
    }
    for layer in layers:
        layer_obj = {
            "layer": layer["layer"],
            "functions": [
                {
                    "name": fqn,
                    "file": str(file_for[fqn].relative_to(REPO)),
                    "unit": fqn.split("::")[-2] if "::" in fqn else "",
                    f"phase{phase_num}_callers": sorted(callers_map[fqn]),
                    f"phase{phase_num}_callees": sorted(callees_map[fqn]),
                    "all_callees": sorted(callees_map[fqn]),
                }
                for fqn in layer["fqns"]
            ],
        }
        if layer.get("cycle"):
            layer_obj["cycle_resolution"] = True
        out["layers"].append(layer_obj)

    out_path = OUT_DIR / f"phase_{phase_num:02d}_topdown_layers.json"
    out_path.write_text(json.dumps(out, indent=2))
    print(f"phase {phase_num} {phase_name}: {len(fqns)} fns / {len(layers)} layers → {out_path.relative_to(REPO)}")


def main(argv):
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    selected = set(int(x) for x in argv[1:]) if argv[1:] else None
    for p in PHASES_JSON["phases"]:
        if selected is not None and p["phase"] not in selected:
            continue
        emit_phase(p["phase"])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
