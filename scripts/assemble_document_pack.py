"""Assemble the anole-document engine pack from the unpacked
LibreOffice tree (E-04, DECISION-3 a).

Layout mirrors the OCR starter pack: manifest.json + sources.json + SPDX
SBOM (via scripts/generate_engine_sbom.py) + PROVENANCE + license notices.
Run from the repo root with a Python that has no extra deps.
"""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SRC = REPO / ".devtools/document-pack-source"
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else REPO / "dist/engine-packs/windows-x86_64/optional/document"

MSI_SHA = "f9877032fd908beb9c0ddf06df4af5c2e85f419c42e14876c4cce5aae5fb2660"
MSI_URL = "https://download.documentfoundation.org/libreoffice/stable/26.2.6/win/x86_64/LibreOffice_26.2.6_Win_x86-64.msi"
VERSION = "26.2.6"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    if OUT.exists():
        import shutil

        shutil.rmtree(OUT)
    OUT.mkdir(parents=True)

    runtime = []
    executables = []
    for path in sorted(SRC.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(SRC).as_posix()
        digest = sha256(path)
        if rel == "program/soffice.com":
            # Executables and runtime_files are disjoint (SBOM contract).
            executables.append({"name": "soffice", "relative_path": rel, "sha256": digest})
        target = OUT / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(path.read_bytes())
        if rel != "program/soffice.com":
            runtime.append({"relative_path": rel, "sha256": digest})

    print(f"packed files: {len(runtime)}", file=sys.stderr)

    licenses_dir = OUT / "licenses"
    licenses_dir.mkdir(exist_ok=True)
    (licenses_dir / "MPL-2.0-NOTICE.txt").write_text(
        "LibreOffice is dual-licensed MPL-2.0 / LGPL-3.0 (Secondary License\n"
        "arrangement). This pack redistributes the substantially unmodified\n"
        "official TDF build; the full license text is at\n"
        "https://www.libreoffice.org/licenses/\n",
        encoding="utf-8",
    )
    (licenses_dir / "VC-RUNTIME-NOTICE.txt").write_text(
        "Microsoft Visual C++ Redistributable runtime DLLs (x64) extracted\n"
        "from the same installer; distributed under the Microsoft Redistributable\n"
        "terms (https://www.microsoft.com/en-us/legal/terms-of-use).\n",
        encoding="utf-8",
    )

    provenance = OUT / "PROVENANCE.txt"
    provenance.write_text(
        "Anole Windows Document optional pack\n"
        "LibreOffice version: 26.2.6 (official TDF Windows x86_64 build)\n"
        "Installer URL: " + MSI_URL + "\n"
        "Installer SHA-256: " + MSI_SHA + "\n"
        "The NSIS/MSI installer was never executed: the MSI database tables\n"
        "(Directory/Component/File) were parsed with pymsi on a Linux host and\n"
        "the payload extracted with 7-Zip; the x64 VC runtime DLLs destined for\n"
        "System32 were placed into program/ instead.\n"
        "Certification status: development/unverified; full transitive license\n"
        "attribution remains a release gate.\n",
        encoding="utf-8",
    )
    runtime.append({"relative_path": "PROVENANCE.txt", "sha256": sha256(provenance)})

    manifest = {
        "schema_version": 1,
        "engine_id": "anole-document",
        "version": VERSION,
        "platform": "windows",
        "architecture": "x86_64",
        "protocol_version": 1,
        "anole_compatibility": {"minimum": "0.1.0", "maximum_exclusive": "0.2.0"},
        "executables": executables,
        "runtime_files": runtime,
        "source": {
            "project_url": "https://www.libreoffice.org/",
            "source_url": "https://www.libreoffice.org/download/source/",
            "source_revision": VERSION,
            "build_configuration": (
                "Official TDF LibreOffice 26.2.6 Win x86_64 MSI "
                f"(sha256={MSI_SHA}); unpacked without execution via "
                "7-Zip + pymsi table reconstruction; x64 VC runtime DLLs "
                "relocated into program/"
            ),
        },
        "licenses": [
            {"spdx": "MPL-2.0", "notice_path": "licenses/MPL-2.0-NOTICE.txt", "source_offer_path": None},
            {"spdx": "LicenseRef-MSVC-Redistributable", "notice_path": "licenses/VC-RUNTIME-NOTICE.txt", "source_offer_path": None},
        ],
        "capabilities": [
            {
                "capability_id": "soffice.office.to-pdf",
                "inputs": ["docx", "xlsx", "pptx", "odt", "ods", "odp", "rtf"],
                "outputs": ["pdf"],
                "operation": "transform",
                "loss_class": "lossy",
                "constraints": {"network_policy": "deny", "isolated_profile": True},
            }
        ],
        "signature": None,
    }

    sources = {
        "schema_version": 1,
        "engine_id": "anole-document",
        "version": VERSION,
        "review_status": "incomplete",
        "artifacts": [
            {
                "name": "LibreOffice 26.2.6 official Windows x86_64 MSI",
                "artifact_type": "binary-distribution",
                "download_url": MSI_URL,
                "sha256": MSI_SHA,
                "source_url": "https://www.libreoffice.org/download/source/",
                "source_revision": VERSION,
                "license_review_status": "incomplete",
            }
        ],
        "completeness_notes": (
            "File-level SPDX inventory covers the declared pack payload "
            "(19k+ files). LibreOffice bundled-component attribution (fonts, "
            "dictionaries, extensions) and legal review remain incomplete; "
            "this pack is not Certified."
        ),
    }

    sources_path = OUT / "sources.json"
    sources_path.write_text(json.dumps(sources, indent=2), encoding="utf-8")
    manifest_path = OUT / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    sbom_path = OUT / "sbom.spdx.json"
    manifest["supply_chain"] = {
        "sbom_path": "sbom.spdx.json",
        "sbom_sha256": "0" * 64,
        "sources_path": "sources.json",
        "sources_sha256": sha256(sources_path),
    }
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    subprocess.run(
        [
            sys.executable,
            str(REPO / "scripts/generate_engine_sbom.py"),
            "--manifest",
            str(manifest_path),
            "--output",
            str(sbom_path),
        ],
        check=True,
    )
    manifest["supply_chain"]["sbom_sha256"] = sha256(sbom_path)
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    subprocess.run(
        [
            sys.executable,
            str(REPO / "scripts/generate_engine_sbom.py"),
            "--manifest",
            str(manifest_path),
            "--verify",
            str(sbom_path),
        ],
        check=True,
    )
    print(f"pack assembled at {OUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
