"""Bundle only real versioned model/texture data, deduplicated by content.

Inputs are existing cached client JARs and the existing distributed game package.
No classes, source, executables, user packs or user paths enter this public data.
"""
import argparse
import hashlib
import json
import pathlib
import zipfile
import zlib


def permitted(name):
    parts = name.split('/')
    return (len(parts) >= 4 and ((parts[0] == 'assets'
            and parts[2] in ('blockstates', 'models', 'textures', 'atlases'))
            or (parts[0] == 'data' and '/worldgen/biome/' in name and name.endswith('.json')))
            and name.endswith(('.json', '.png', '.mcmeta'))
            and '..' not in parts and '\\' not in name)


def main():
    root = pathlib.Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument('--cache', type=pathlib.Path, default=root / '.tmp/game-data/version-runtime')
    parser.add_argument('--output', type=pathlib.Path, default=root / 'pack-in/layer-resources')
    parser.add_argument('--extra-jar', action='append', default=[])
    parser.add_argument('--package', action='append', default=[])
    args = parser.parse_args()
    extra = dict(value.split('=', 1) for value in args.extra_jar)
    packages = dict(value.split('=', 1) for value in args.package)
    versions = json.loads((root / 'pack-in/game-data/default/derived/blockdb/versions.json').read_text('utf-8-sig'))
    args.output.mkdir(parents=True, exist_ok=True)
    temporary = args.output / 'assets.zip.tmp'
    catalog = {'schema': 1, 'versions': {}}
    blobs = set()
    with zipfile.ZipFile(temporary, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as output:
        for version in versions:
            resources = {}
            package = pathlib.Path(packages[version]) if version in packages else None
            source = pathlib.Path(extra.get(version, args.cache / version / 'client.jar'))
            if package:
                manifest = json.loads((package / 'manifest.json').read_text('utf-8-sig'))
                if manifest['game_version'] != version:
                    raise ValueError(f'Game package does not match requested version {version}')
                source_hash = manifest['source_jar_sha256']
                names = sorted(name for name in json.loads((package / 'index.json').read_text('utf-8-sig')) if permitted(name))
                entries = ((name, (package / name).read_bytes()) for name in names)
            else:
                source_hash = hashlib.file_digest(source.open('rb'), 'sha256').hexdigest()
                archive = zipfile.ZipFile(source)
                if 'version.json' in archive.namelist():
                    actual = json.loads(archive.read('version.json'))
                    if actual.get('name', actual.get('id')) != version:
                        raise ValueError(f'Client JAR does not match requested version {version}: {actual.get("id")}')
                names = sorted(name for name in archive.namelist() if permitted(name))
                entries = ((name, archive.read(name)) for name in names)
            for name, data in entries:
                key = hashlib.sha256(data).hexdigest()
                resources[name] = key
                if key not in blobs:
                    info = zipfile.ZipInfo(key, (1980, 1, 1, 0, 0, 0))
                    info.compress_type = zipfile.ZIP_DEFLATED
                    output.writestr(info, data, compresslevel=9)
                    blobs.add(key)
            if not package:
                archive.close()
            if not resources:
                raise ValueError(f'No real model/texture assets in version {version}')
            catalog['versions'][version] = {'source_sha256': source_hash, 'resources': resources}
    identity = hashlib.sha256(json.dumps(catalog, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    catalog['identity'] = identity
    index = json.dumps(catalog, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()
    index_path = args.output / 'index.json.z.tmp'
    index_path.write_bytes(zlib.compress(index, 9))
    temporary.replace(args.output / 'assets.zip')
    index_path.replace(args.output / 'index.json.z')
    print(json.dumps({'versions': len(versions), 'resource_entries': sum(len(v['resources']) for v in catalog['versions'].values()),
                      'unique_blobs': len(blobs), 'identity': identity,
                      'archive_bytes': (args.output / 'assets.zip').stat().st_size,
                      'index_bytes': (args.output / 'index.json.z').stat().st_size}))


if __name__ == '__main__':
    main()
