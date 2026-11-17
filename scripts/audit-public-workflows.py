#!/usr/bin/env python3
"""Run real public-data workflows in temporary databases; requires developer SDKs."""
import json
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
crate = root / 'target/public-workflow-qa'
crate.mkdir(parents=True, exist_ok=True)
manifest = '''[package]
name="pcc-public-workflow-audit"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
command-center-core={path=CORE}
serde_json="1"
tempfile="3"
uuid={version="1",features=["v4"]}
base64="0.22"
sha2="0.10"
[[bin]]
name="audit"
path=SOURCE
'''.replace('CORE', json.dumps(str(root / 'crates/core'))).replace('SOURCE', json.dumps(str(root / 'scripts/public-workflow-audit.rs')))
(crate / 'Cargo.toml').write_text(manifest)
subprocess.run(['node', 'scripts/audit-discovery-cache.cjs'], cwd=root, check=True)
subprocess.run(['cargo', 'run', '--manifest-path', str(crate / 'Cargo.toml'), '--offline', '--',
                'target/final-dmg-audit/workflow-input.json', 'target/final-dmg-audit/workflows.json'],
               cwd=root, env={**os.environ, 'CARGO_NET_OFFLINE': 'true',
                              'CARGO_TARGET_DIR': str(root / 'target/public-workflow-qa-build')}, check=True)
