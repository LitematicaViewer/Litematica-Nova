"""Bundle only real versioned model/texture data, deduplicated by content.

Inputs are existing cached client JARs and the existing distributed game package.
No classes, source, executables, user packs or user paths enter this public data.
"""
import argparse
import hashlib
import json
import pathlib
import tempfile
import zipfile
import zlib


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()


def compact_index(source, output):
    """Encode an audited v1 catalog + ZIP without changing any resource or logical identity."""
    old_zip = source.with_name('assets.zip')
    if output.resolve() == source.resolve().parent:
        raise ValueError('Distributable output must not overwrite the source catalog')
    catalog = json.loads(zlib.decompress(source.read_bytes()))
    identity = catalog.pop('identity')
    if catalog['schema'] != 1 or hashlib.sha256(canonical(catalog)).hexdigest() != identity:
        raise ValueError('Invalid source layer catalog or identity')
    records = catalog['versions']
    names = sorted({name for record in records.values() for name in record['resources']})
    blobs = sorted({sha for record in records.values() for sha in record['resources'].values()})
    if not names or not blobs or not all(permitted(name) for name in names):
        raise ValueError('Invalid layer resource names')
    positions = {name: i for i, name in enumerate(names)}
    blob_ids = {sha: i for i, sha in enumerate(blobs, start=1)}  # zero is an absent resource
    versions, maps, known = {}, [], {}
    for version, record in sorted(records.items()):
        row = [0] * len(names)
        for name, sha in record['resources'].items():
            row[positions[name]] = blob_ids[sha]
        row = tuple(row)
        index = known.get(row)
        if index is None:
            index = known[row] = len(maps)
            maps.append(row)
        versions[version] = [record['source_sha256'], index]

    # Six independent complete anchors. Greedy medoids minimize the remaining
    # number of overrides for this data set; no table ever references a delta.
    count = len(maps)
    distances = [[0] * count for _ in maps]
    for i in range(count):
        for j in range(i):
            distances[i][j] = distances[j][i] = sum(a != b for a, b in zip(maps[i], maps[j]))
    anchors, nearest = [], [len(names) + 1] * count
    for _ in range(min(6, count)):
        chosen = min((i for i in range(count) if i not in anchors),
                     key=lambda i: sum(min(d, distance) for d, distance in zip(nearest, distances[i])))
        anchors.append(chosen)
        nearest = [min(d, distance) for d, distance in zip(nearest, distances[chosen])]
    tables = []
    for i, row in enumerate(maps):
        anchor = min(range(len(anchors)), key=lambda a: distances[i][anchors[a]])
        changes = [[pos, new] for pos, (old, new) in enumerate(zip(maps[anchors[anchor]], row)) if old != new]
        tables.append([anchor, changes])
    compact = {'schema': 3, 'identity': identity, 'versions': versions, 'names': names,
               'blobs': blobs, 'anchors': [maps[i] for i in anchors], 'tables': tables}
    output.mkdir(parents=True, exist_ok=True)
    # Both files are generated and validated before publication. The build
    # caller records their exact hashes together; source assets remain untouched.
    with tempfile.TemporaryDirectory(prefix='.stage-', dir=output) as directory:
        stage = pathlib.Path(directory)
        with zipfile.ZipFile(old_zip) as archive, zipfile.ZipFile(stage / 'assets.zip', 'w') as short:
            entries = archive.infolist()
            if len(entries) != len(blobs) or {e.filename for e in entries} != set(blobs):
                raise ValueError('Layer archive members do not match the index')
            for entry in entries:
                data = archive.read(entry)
                if hashlib.sha256(data).hexdigest() != entry.filename:
                    raise ValueError(f'Layer archive blob checksum mismatch: {entry.filename}')
                info = zipfile.ZipInfo(str(blob_ids[entry.filename]), (1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                short.writestr(info, data, compresslevel=9)
        (stage / 'index.json.z').write_bytes(zlib.compress(canonical(compact), 9))
        for file in ['assets.zip', 'index.json.z']:
            (stage / file).replace(output / file)
    print(json.dumps({'versions':len(versions), 'tables':len(tables), 'anchors':len(anchors),
                      'paths':len(names), 'blobs':len(blobs), 'index_bytes':(output/'index.json.z').stat().st_size,
                      'archive_bytes':(output/'assets.zip').stat().st_size, 'identity':identity}))


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
    parser.add_argument('--compact-index', type=pathlib.Path, help='Write the distributable v3 index and compact ZIP from an existing audited v1 catalog/ZIP')
    args = parser.parse_args()
    if args.compact_index:
        return compact_index(args.compact_index, args.output)
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
                archive = zipfile.ZipFile(package / 'resources.jar')
                entries = ((name, archive.read(name)) for name in names)
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
