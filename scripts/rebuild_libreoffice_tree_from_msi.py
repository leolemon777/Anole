"""Rebuild the LibreOffice application tree from msi tables (pymsi).

Run on the Linux executor (pymsi + the 7z flat extraction in /tmp/fw-lo-pack).
Windows host never executes the installer.

Flat files from the 7z extraction are named by File.id; the destination path
comes from Component.directory walking _parent links. Only the application
root subtree (the LibreOffice install folder) moves into the tree; System
Folder components (VC runtime to System32) and menu/desktop folders are
skipped.
"""
import shutil
import sys
from pathlib import Path

from pymsi.package import Package
from pymsi.msi import Msi

W = Path("/tmp/fw-lo-pack")
pkg = Package(W / "LibreOffice_26.2.6_Win_x86-64.msi")
m = Msi(pkg)

memo = {}


def dir_path(rec):
    """Resolve a Directory record to its path list from the install root."""
    key = rec.id if hasattr(rec, "id") else str(rec)
    if key in memo:
        return memo[key]
    parent = getattr(rec, "parent", None)
    name = getattr(rec, "name", None) or ""
    if parent is None:
        memo[key] = []
    else:
        pkey = parent.id if hasattr(parent, "id") else None
        if pkey == "TARGETDIR":
            memo[key] = [name] if name and name != "." else []
        else:
            memo[key] = dir_path(parent) + ([name] if name and name != "." else [])
    return memo[key]


def long_name(raw):
    if not raw:
        return raw
    raw = raw.split("|")[-1]
    # MSI short-name padding: trailing '_' on an extension means the real
    # extension was longer than 3 characters.
    if raw.endswith("_") and "." in raw:
        raw = raw[:-1] + "_"
    return raw


tree = W / "tree"
if tree.exists():
    shutil.rmtree(tree)
tree.mkdir(parents=True)

flat = W / "msi-raw"
moved = skipped_system = 0
seen_dirs = {}

for comp_key, comp in m.components.items():
    drec = getattr(comp, "directory", None)
    if drec is None:
        continue
    chain = dir_path(drec)
    root = chain[0] if chain else ""
    # Application payload lives under the LibreOffice install folder; skip
    # anything destined for Windows system/menu/desktop locations.
    if not root or root.lower() in (
        "system",
        "system64folder",
        "windows",
        "programmenufolder",
        "desktopfolder",
        "commonfilesfolder",
        "commonappdatafolder",
        "tempfolder",
    ) or root.lower().startswith(("common", "windows", "system")):
        skipped_system += len(getattr(comp, "files", {}) or {})
        continue
    rel_dir = Path(*chain)
    for file_key, frec in getattr(comp, "files", {}).items():
        src = flat / file_key
        if not src.is_file():
            continue
        dest = tree / rel_dir / long_name(getattr(frec, "name", None) or file_key)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(src), str(dest))
        moved += 1

print(f"moved={moved} skipped_system_components={skipped_system}", file=sys.stderr)
for probe in (
    "program/soffice.com",
    "program/soffice.bin",
    "program/soffice.exe",
    "share",
):
    print(probe, (tree / probe).exists(), file=sys.stderr)
leftover = [p.name for p in flat.iterdir() if p.is_file() and not p.name.endswith(".cfs")]
print("unmoved leftovers:", len(leftover), leftover[:8], file=sys.stderr)
