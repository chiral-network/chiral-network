#!/usr/bin/env python3
"""
Per-function source extractor for the FM-Agent pipeline.

Reads `fm_agent/phases.json` and, for every source file listed under
each phase's modules, extracts each free-standing or impl-method
function into its own file under
`fm_agent/extracted_functions/<src_dir>/<src_file_with_dot_replaced>/<fn_name>.<ext>`.

Rust-only for now (the project's only listed language). Recognises:
  - `pub fn`, `fn`, `pub async fn`, `async fn`, `pub(crate) fn`, etc.
  - methods inside `impl Type { ... }` and `impl Trait for Type { ... }`
  - skips `extern "C" fn`, `unsafe fn` body? — these are also captured
  - skips functions inside `#[cfg(test)] mod tests { ... }`
  - skips macro_rules! blocks

Body extraction uses brace-balanced scanning with awareness of strings,
chars, line comments, block comments, and raw strings (`r#"..."#`).

Output: one file per function. The file contains JUST the function's
source code (signature line through closing brace), no surrounding
context. The original source file is NEVER modified.

Usage:
    python3 fm_agent/extractor.py            # all phases
    python3 fm_agent/extractor.py 1 3 8      # specific phases
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PHASES = json.loads((REPO / "fm_agent" / "phases.json").read_text())

OUT_ROOT = REPO / "fm_agent" / "extracted_functions"


# -----------------------------------------------------------------------------
# Lexer-aware brace scanning
# -----------------------------------------------------------------------------

def find_matching_brace(src: str, start: int) -> int:
    """Given that src[start] == '{', return the index of its matching '}'.
    Skips braces inside strings, chars, comments, and raw strings."""
    assert src[start] == "{"
    i = start
    depth = 0
    n = len(src)
    while i < n:
        c = src[i]
        # line comment
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            i = n if j == -1 else j + 1
            continue
        # block comment (Rust supports nesting)
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            i += 2
            block_depth = 1
            while i < n and block_depth > 0:
                if src[i] == "/" and i + 1 < n and src[i + 1] == "*":
                    block_depth += 1
                    i += 2
                elif src[i] == "*" and i + 1 < n and src[i + 1] == "/":
                    block_depth -= 1
                    i += 2
                else:
                    i += 1
            continue
        # raw string r#..."..."#..
        if c == "r" and i + 1 < n and src[i + 1] in ('"', "#"):
            j = i + 1
            hash_count = 0
            while j < n and src[j] == "#":
                hash_count += 1
                j += 1
            if j < n and src[j] == '"':
                # find closing "###...
                close = '"' + ("#" * hash_count)
                k = src.find(close, j + 1)
                if k == -1:
                    return -1
                i = k + len(close)
                continue
        # byte string b"..."
        if c == "b" and i + 1 < n and src[i + 1] == '"':
            i += 1
            c = '"'
        # ordinary string
        if c == '"':
            i += 1
            while i < n:
                if src[i] == "\\":
                    i += 2
                    continue
                if src[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        # char literal — skip with simple heuristic; lifetimes are 'a NOT followed by '
        if c == "'":
            # could be lifetime: 'ident
            if i + 1 < n and (src[i + 1].isalpha() or src[i + 1] == "_"):
                # advance past lifetime ident; if followed by ' then it's a char literal
                j = i + 1
                while j < n and (src[j].isalnum() or src[j] == "_"):
                    j += 1
                if j < n and src[j] == "'":
                    # actually a char literal like 'a'
                    i = j + 1
                else:
                    i = j
                continue
            # char literal
            i += 1
            while i < n:
                if src[i] == "\\":
                    i += 2
                    continue
                if src[i] == "'":
                    i += 1
                    break
                i += 1
            continue
        if c == "{":
            depth += 1
            i += 1
            continue
        if c == "}":
            depth -= 1
            i += 1
            if depth == 0:
                return i  # one past closing brace
            continue
        i += 1
    return -1


def find_block_end(src: str, brace_index: int) -> int:
    end = find_matching_brace(src, brace_index)
    return end


# -----------------------------------------------------------------------------
# Function discovery
# -----------------------------------------------------------------------------

# Match a Rust function declaration. The "{" or ";" ends the signature; bodies
# start at "{". We anchor on `fn IDENT` and then walk back to capture
# attributes / visibility / async / unsafe / extern, and forward to find the
# brace.
FN_PATTERN = re.compile(
    r"""
    (?P<sig_start>
        (?:^[ \t]*(?:\#\[[^\n]*\][ \t]*\n)*)        # attributes lines
        [ \t]*
        (?:pub(?:\([^)]+\))?[ \t]+)?                 # visibility
        (?:default[ \t]+)?
        (?:const[ \t]+)?
        (?:async[ \t]+)?
        (?:unsafe[ \t]+)?
        (?:extern[ \t]+(?:"[^"]+"[ \t]+)?)?
    )
    fn[ \t]+(?P<name>[A-Za-z_][A-Za-z0-9_]*)
    """,
    re.VERBOSE | re.MULTILINE,
)


def strip_test_modules(src: str) -> str:
    """Replace bodies of `#[cfg(test)] mod ... { ... }` blocks with whitespace
    so functions inside tests are not extracted."""
    out = []
    i = 0
    n = len(src)
    pat = re.compile(r"#\[cfg\(test\)\][ \t\r\n]*(?:pub[ \t]+)?mod[ \t]+\w+[ \t]*\{")
    while i < n:
        m = pat.search(src, i)
        if not m:
            out.append(src[i:])
            break
        out.append(src[i:m.start()])
        brace = m.end() - 1
        end = find_matching_brace(src, brace)
        if end == -1:
            out.append(src[m.start():])
            break
        # replace mod body with same-length whitespace to preserve offsets
        out.append(" " * (end - m.start()))
        i = end
    return "".join(out)


def extract_functions(src: str):
    """Yield (name, body_text) for every top-level or impl-method function
    in `src`, EXCLUDING #[cfg(test)] mods."""
    cleaned = strip_test_modules(src)
    n = len(cleaned)
    seen = set()
    for m in FN_PATTERN.finditer(cleaned):
        name = m.group("name")
        # find the start of the signature (rewind past attributes / leading whitespace
        # to the first non-blank-line-start)
        sig_start = m.start("sig_start")
        # skip blank-line-only prefix
        while sig_start < m.start() and cleaned[sig_start] in " \t":
            sig_start += 1
        # find body brace or trait-method semicolon
        # walk forward from end of name, balancing < > parens to find { or ;
        i = m.end("name")
        paren_depth = 0
        angle_depth = 0
        while i < n:
            c = cleaned[i]
            if c == "(":
                paren_depth += 1
            elif c == ")":
                paren_depth -= 1
            elif c == "<" and paren_depth == 0:
                # could be generics or comparator; treat as generics if preceded
                # by ident or > or whitespace after fn-context
                angle_depth += 1
            elif c == ">" and angle_depth > 0:
                angle_depth -= 1
            elif c == "{" and paren_depth == 0 and angle_depth == 0:
                break
            elif c == ";" and paren_depth == 0 and angle_depth == 0:
                # trait method declaration without body — skip
                i = -1
                break
            elif c == "/" and i + 1 < n and cleaned[i + 1] == "/":
                j = cleaned.find("\n", i)
                i = n if j == -1 else j + 1
                continue
            elif c == "/" and i + 1 < n and cleaned[i + 1] == "*":
                # skip block comment
                i += 2
                bd = 1
                while i < n and bd > 0:
                    if cleaned[i] == "/" and i + 1 < n and cleaned[i + 1] == "*":
                        bd += 1
                        i += 2
                    elif cleaned[i] == "*" and i + 1 < n and cleaned[i + 1] == "/":
                        bd -= 1
                        i += 2
                    else:
                        i += 1
                continue
            i += 1
        if i == -1 or i >= n:
            continue
        end = find_matching_brace(cleaned, i)
        if end == -1:
            continue
        body = cleaned[sig_start:end]
        # de-collide same-named methods across impls — append a counter
        key = name
        c = 0
        while key in seen:
            c += 1
            key = f"{name}__{c}"
        seen.add(key)
        yield key, body


# -----------------------------------------------------------------------------
# Main
# -----------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    selected = set(int(x) for x in argv[1:]) if argv[1:] else None
    OUT_ROOT.mkdir(parents=True, exist_ok=True)

    total_files = 0
    total_fns = 0

    for phase in PHASES["phases"]:
        if selected is not None and phase["phase"] not in selected:
            continue
        for module in phase["modules"]:
            for src_path_rel in module["source_files"]:
                src_path = REPO / src_path_rel
                if not src_path.exists():
                    print(f"  MISSING: {src_path_rel}", file=sys.stderr)
                    continue
                src = src_path.read_text()
                # output dir: <parent_dir>/<filename_with_ext_dot_replaced>
                parts = list(Path(src_path_rel).parts)
                fname = parts[-1]
                stem, ext = fname.rsplit(".", 1)
                out_dir = OUT_ROOT.joinpath(*parts[:-1], f"{stem}-{ext}")
                out_dir.mkdir(parents=True, exist_ok=True)
                count = 0
                for name, body in extract_functions(src):
                    (out_dir / f"{name}.{ext}").write_text(body + "\n")
                    count += 1
                total_files += 1
                total_fns += count
                print(f"  phase {phase['phase']} {src_path_rel} → {count} fns")

    print(f"\nDone: {total_fns} functions extracted from {total_files} files into {OUT_ROOT}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
