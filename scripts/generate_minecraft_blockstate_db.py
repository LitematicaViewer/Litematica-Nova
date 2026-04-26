import os
import json
import zipfile
import argparse
from pathlib import Path

def stable_value_sort(values):
    def key(value):
        text = str(value)
        if text.isdigit():
            return (0, int(text), text)
        return (1, text)
    return sorted({str(v) for v in values}, key=key)

def parse_variants(variants):
    properties = {}
    for key in variants.keys():
        if not key:
            continue
        # key example: "facing=north,half=bottom"
        parts = key.split(',')
        for part in parts:
            if '=' in part:
                k, v = part.split('=', 1)
                if k not in properties:
                    properties[k] = set()
                properties[k].add(v)
    return properties

def parse_when(when, properties):
    if not when:
        return
    
    if "OR" in when:
        for cond in when["OR"]:
            parse_when(cond, properties)
        return
    
    if "AND" in when:
        for cond in when["AND"]:
            parse_when(cond, properties)
        return
    
    for k, v in when.items():
        if k not in properties:
            properties[k] = set()
        
        # value can be "north|south"
        if isinstance(v, str):
            for val in v.split('|'):
                properties[k].add(val)
        else:
            # handle cases like booleans if they exist
            properties[k].add(str(v).lower())

def extract_blockstates_from_jar(jar_path):
    blocks = {}
    total_blocks = 0
    with zipfile.ZipFile(jar_path, 'r') as jar:
        for name in jar.namelist():
            if name.startswith('assets/minecraft/blockstates/') and name.endswith('.json'):
                total_blocks += 1
                block_id = f"minecraft:{Path(name).stem}"
                try:
                    content = json.loads(jar.read(name))
                except json.JSONDecodeError:
                    print(f"Warning: Failed to parse {name}")
                    continue
                
                block_properties = {}
                
                if "variants" in content:
                    props = parse_variants(content["variants"])
                    for k, v in props.items():
                        if k not in block_properties:
                            block_properties[k] = set()
                        block_properties[k].update(v)
                
                if "multipart" in content:
                    for part in content["multipart"]:
                        if "when" in part:
                            parse_when(part["when"], block_properties)
                
                # Convert sets to sorted lists
                sorted_props = {}
                default_props = {}
                
                has_empty_variant = "variants" in content and "" in content["variants"]
                
                for k in sorted(block_properties.keys()):
                    vals = sorted(list(block_properties[k]))
                    sorted_props[k] = vals
                    if not has_empty_variant and vals:
                        default_props[k] = vals[0]
                
                blocks[block_id] = {
                    "properties": sorted_props,
                    "default_properties": default_props
                }
    
    return blocks, total_blocks

def apply_overrides(blocks, override_path):
    summary = {
        "override_blocks_applied": 0,
        "override_properties_added": 0,
        "override_values_added": 0,
    }
    if not override_path.is_file():
        return summary

    with open(override_path, "r", encoding="utf-8") as f:
        overrides = json.load(f)

    for block_id, override in overrides.items():
        if block_id not in blocks:
            blocks[block_id] = {
                "properties": {},
                "default_properties": {},
            }
        summary["override_blocks_applied"] += 1

        block = blocks[block_id]
        properties = block.setdefault("properties", {})
        defaults = block.setdefault("default_properties", {})

        for prop_name, override_values in override.get("properties", {}).items():
            existing_values = set(str(v) for v in properties.get(prop_name, []))
            if prop_name not in properties:
                summary["override_properties_added"] += 1
            before_count = len(existing_values)
            existing_values.update(str(v) for v in override_values)
            summary["override_values_added"] += max(0, len(existing_values) - before_count)
            properties[prop_name] = stable_value_sort(existing_values)

        force_default = bool(override.get("force_default"))
        for prop_name, default_value in override.get("default_properties", {}).items():
            if force_default or prop_name not in defaults:
                defaults[prop_name] = str(default_value)

    return summary

def main():
    parser = argparse.ArgumentParser(description="Generate Minecraft BlockState database from JAR")
    parser.get_default('--version-dir')
    parser.add_argument("--version-dir", type=str, required=True, help="Directory containing Minecraft versions")
    parser.add_argument("--jar", type=str, help="Path to Minecraft JAR (optional)")
    parser.add_argument("--output", type=str, help="Output JSON path (optional)")
    parser.add_argument("--version-id", type=str, help="Minecraft version ID (optional)")
    parser.add_argument("--override", type=str, help="Override JSON path (optional)")
    
    args = parser.parse_args()
    
    version_dir = Path(args.version_dir)
    version_id = args.version_id or version_dir.name
    
    jar_path = args.jar
    if not jar_path:
        # Try default locations
        candidate1 = version_dir / f"{version_id}.jar"
        if candidate1.is_file():
            jar_path = candidate1
        else:
            # Find first jar in version_dir
            jars = list(version_dir.glob("*.jar"))
            if jars:
                jar_path = jars[0]
            else:
                print(f"Error: No JAR file found in {version_dir}")
                return
    else:
        jar_path = Path(jar_path)
        
    if not jar_path.is_file():
        print(f"Error: JAR file not found at {jar_path}")
        return

    output_path = args.output
    if not output_path:
        output_dir = Path("data/minecraft_blockstates")
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / f"{version_id}.json"
    else:
        output_path = Path(output_path)
        output_path.parent.mkdir(parents=True, exist_ok=True)

    print(f"Extracting from: {jar_path}")
    blocks, total_count = extract_blockstates_from_jar(jar_path)
    override_path = Path(args.override) if args.override else Path("data/minecraft_blockstates/overrides") / f"{version_id}.json"
    override_summary = apply_overrides(blocks, override_path)
    
    db = {
        "version_id": version_id,
        "source": {
            "version_dir": str(version_dir.resolve()),
            "jar": str(jar_path.resolve())
        },
        "blocks": blocks
    }
    
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(db, f, indent=2, ensure_ascii=False)
        
    # Stats
    blocks_with_states = sum(1 for b in blocks.values() if b["properties"])
    total_properties = sum(len(b["properties"]) for b in blocks.values())
    
    print(f"Database generated: {output_path}")
    print(f"Total blocks: {total_count}")
    print(f"Blocks with states: {blocks_with_states}")
    print(f"Total property keys: {total_properties}")
    print(f"Override file: {override_path if override_path.is_file() else 'not found'}")
    print(f"override_blocks_applied: {override_summary['override_blocks_applied']}")
    print(f"override_properties_added: {override_summary['override_properties_added']}")
    print(f"override_values_added: {override_summary['override_values_added']}")

if __name__ == "__main__":
    main()
