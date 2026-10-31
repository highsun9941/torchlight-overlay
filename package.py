"""Package a verified build without user data, debug symbols, captures or machine paths."""
from pathlib import Path
import hashlib
import json
import zipfile

ROOT = Path(__file__).resolve().parent
BUILD = ROOT / 'release' / 'TorchlightHelper-KO'
ARCHIVE = ROOT / 'release' / 'TorchlightHelper-KO-1.1.0-win-x64.zip'
BLOCKED = {'user-data', 'dataset', 'reports', 'qa'}

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    for name in ['TorchlightHelper.exe', 'vorax-overlay.exe', 'README.ko.md', 'THIRD_PARTY_NOTICES.md', 'models/ocr/rec_korean.onnx']:
        if not (BUILD/name).is_file():
            raise SystemExit(f'Missing build artifact: {name}')
    dependencies = BUILD/'licenses/dependencies'
    for cache, kind in [(ROOT/'.nuget', 'nuget'), (ROOT/'.toolchain/cargo/registry/src', 'rust')]:
        if not cache.exists():
            continue
        for source in cache.rglob('*'):
            if not source.is_file(): continue
            n=source.name.lower()
            if not (n.startswith(('license', 'copying', 'copyright', 'notice')) and source.suffix.lower() in ('', '.txt', '.md', '.rst')):
                continue
            if source.stat().st_size > 300_000: continue
            target=dependencies/kind/source.relative_to(cache)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source.read_bytes())
    files=[]
    for source in sorted(BUILD.rglob('*')):
        if not source.is_file(): continue
        rel=source.relative_to(BUILD)
        if any(p.lower() in BLOCKED for p in rel.parts): continue
        if source.suffix.lower() in ('.pdb', '.log', '.zip', '.dmp'): continue
        if source.name.lower() in ('settings.json', 'status.json', 'command.txt', 'manifest.json'): continue
        files.append((source,rel))
    local_paths = [str(ROOT), ROOT.as_posix()]
    markers = [p.encode(encoding) for p in local_paths for encoding in ('utf-8', 'utf-16-le')]
    for source, rel in files:
        data = source.read_bytes()
        if any(marker in data for marker in markers):
            raise SystemExit(f'Local build path in release artifact: {rel}')
    manifest=[{'path':rel.as_posix(),'bytes':p.stat().st_size,'sha256':digest(p)} for p,rel in files]
    encoded=json.dumps(manifest,ensure_ascii=False,indent=2).encode('utf-8')
    (BUILD/'manifest.json').write_bytes(encoded)
    with zipfile.ZipFile(ARCHIVE,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as archive:
        for source,rel in files:
            archive.write(source,'TorchlightHelper-KO/'+rel.as_posix())
        archive.writestr('TorchlightHelper-KO/manifest.json',encoded)
    with zipfile.ZipFile(ARCHIVE) as archive:
        assert archive.testzip() is None
        for path in archive.namelist():
            assert not any(p in BLOCKED for p in Path(path).parts),path
            assert not path.endswith(('.log','.pdb','.dmp')),path
    checksum=digest(ARCHIVE)
    ARCHIVE.with_suffix('.zip.sha256').write_text(checksum+'  '+ARCHIVE.name+'\n',encoding='ascii')
    print(json.dumps({'archive':str(ARCHIVE),'files':len(files)+1,'bytes':ARCHIVE.stat().st_size,'sha256':checksum,'private_files':0},indent=2))

if __name__=='__main__': main()
