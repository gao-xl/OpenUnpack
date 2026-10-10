"""Build and verify a self-contained Windows plugin ZIP without credential files."""
import argparse
import hashlib
import json
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED

parser = argparse.ArgumentParser()
parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parent.parent)
parser.add_argument('--output', type=Path)
args = parser.parse_args()
root = args.root.resolve()
manifest = json.loads((root / '.codex-plugin/plugin.json').read_text(encoding='utf-8'))
pkg = json.loads((root / 'package.json').read_text(encoding='utf-8'))
assert manifest['version'] == pkg['version']
payload = ['.codex-plugin', '.mcp.json', 'assets', 'skills', 'src', 'scripts', 'test', 'node_modules',
           'config.json', 'package.json', 'package-lock.json', 'run.ps1', 'set-credential.ps1', 'setup.ps1',
           'README.md', 'VALIDATION.md']
files = []
for item in payload:
    target = root / item
    assert target.exists(), f'Missing payload: {item}'
    files.extend(p for p in (target.rglob('*') if target.is_dir() else [target]) if p.is_file())
assert not any(p.suffix.lower() == '.dpapi' or p.name.startswith('.env') for p in files)
output = args.output or root / 'dist' / f'gemini-subagent-{pkg["version"]}-windows.zip'
output.parent.mkdir(parents=True, exist_ok=True)
with ZipFile(output, 'w', ZIP_DEFLATED) as archive:
    for file in sorted(files):
        archive.write(file, file.relative_to(root).as_posix())
with ZipFile(output) as archive:
    assert archive.testzip() is None
    assert {'.codex-plugin/plugin.json', '.mcp.json', 'setup.ps1', 'scripts/check-plugin.mjs',
            'test/deployment.test.mjs'}.issubset(archive.namelist())
digest = hashlib.sha256(output.read_bytes()).hexdigest()
output.with_suffix('.zip.sha256').write_text(f'{digest}  {output.name}\n', encoding='utf-8')
print(json.dumps({'path': str(output.resolve()), 'files': len(files), 'bytes': output.stat().st_size,
                  'sha256': digest, 'version': pkg['version']}, ensure_ascii=False))
