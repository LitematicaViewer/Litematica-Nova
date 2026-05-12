from __future__ import annotations

import hashlib
import json
import os
import re
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


REPO_ROOT = Path(__file__).resolve().parents[1]
DB_DIR = REPO_ROOT / ".tmp" / "assistant"
DB_PATH = DB_DIR / "lba_assistant_index.sqlite"

DOC_FILES = [
    REPO_ROOT / "README.md",
    REPO_ROOT / "docs" / "ARCHITECTURE.md",
    REPO_ROOT / "docs" / "DEVELOPMENT.md",
]

CODE_ROOTS = [
    REPO_ROOT / "tools" / "viewer-core" / "src",
    REPO_ROOT / "scripts",
    REPO_ROOT / "desktop-nova" / "src",
    REPO_ROOT / "desktop-nova" / "src-tauri" / "src",
    REPO_ROOT / "docs",
]

TEXT_EXTENSIONS = {
    ".md",
    ".txt",
    ".rs",
    ".py",
    ".ps1",
    ".bat",
    ".toml",
    ".json",
    ".html",
    ".js",
    ".css",
}

RUST_SYMBOL_RE = re.compile(
    r"^\s*(?:pub\s+)?(?P<kind>fn|struct|enum|trait|mod)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
RUST_IMPL_RE = re.compile(r"^\s*impl(?:<[^>]+>)?\s+(?P<name>[A-Za-z_][A-Za-z0-9_:<>]*)")
PY_SYMBOL_RE = re.compile(
    r"^\s*(?P<kind>class|def)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
ENV_FLAG_RE = re.compile(r"\bLBA_[A-Z0-9_]+\b")
BLOCK_ID_RE = re.compile(r"\bminecraft:[a-z0-9_]+\b")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.*)$")


DOC_ABSTRACTS = {
    "README.md": "项目入口，说明 desktop-nova 主线、运行方式、关键目录和常用命令。",
    "ARCHITECTURE.md": "desktop-nova 分层架构、Tauri/Rust 后端桥接、AppData、viewer 关系和禁区。",
    "DEVELOPMENT.md": "开发验证命令、projection plan、AI、RedenMC、BlockState、清理政策和故障排查。",
}


MODULE_ROLE_HINTS = {
    "tools/viewer-core/src/nbt.rs": ["litematic_parse", "nbt_decode", "palette_bits"],
    "tools/viewer-core/src/visual.rs": ["catalog_build", "visual_output", "metadata"],
    "tools/viewer-core/src/mesh.rs": ["normal_fast_mesh", "culling", "chunk_scene"],
    "tools/viewer-core/src/full_mode.rs": ["full_mode_shared", "material_cache"],
    "tools/viewer-core/src/full_mode_v2.rs": ["full_mode_v2", "typed_model_chain", "block_family"],
    "tools/viewer-core/src/native_viewer.rs": ["native_window", "wgpu", "preview_output"],
    "tools/viewer-core/src/analyze.rs": ["analysis", "palette_frequency"],
    "desktop-nova/src/services/backend.ts": ["tauri_bridge", "desktop_backend"],
    "desktop-nova/ui/windows/main/pages/generate/GeneratePage.tsx": ["ai_projection", "plan_generation"],
}


PROJECT_STATE = {
    "snapshot_date": "2026-04-23",
    "primary_focus": "desktop-nova is the active UI; Rust viewer-core is the active backend/viewer. Legacy PySide6 UI has been removed.",
    "stable_chain": [
        ".litematic -> gzip -> NBT parsing",
        "scene/catalog generation",
        "normal / fast_experimental / full mode entry wiring",
        "native viewer popup and preview output",
    ],
    "high_risk_areas": [
        "tools/viewer-core/src/full_mode_v2.rs shared typed UV / rotation / transform sections",
        "tools/viewer-core/src/mesh.rs preserve-neighbor-faces and culling semantics",
        "chest family logic if task scope is not explicitly chest-specific",
    ],
    "active_family_notes": {
        "piston": "Static piston family is in typed model chain and still being refined around UV/orientation.",
        "chest": "Current narrow issue is east/west double chest front texture identity downstream sampling chain; do not widen scope.",
        "hopper": "Typed model path already in use; recent fixes centered on face UV/material mapping.",
    },
}


@dataclass
class SymbolRecord:
    path: str
    lang: str
    kind: str
    name: str
    line_start: int
    signature: str
    qualname: str
    tags: list[str]


def sha1_text(text: str) -> str:
    return hashlib.sha1(text.encode("utf-8", errors="ignore")).hexdigest()


def rel(path: Path) -> str:
    return path.relative_to(REPO_ROOT).as_posix()


def read_text(path: Path) -> str:
    for encoding in ("utf-8", "utf-8-sig", "gb18030", "cp936", "latin-1"):
        try:
            return path.read_text(encoding=encoding)
        except UnicodeDecodeError:
            continue
    return path.read_text(encoding="utf-8", errors="ignore")


def iter_text_files() -> Iterable[Path]:
    yielded: set[Path] = set()
    for path in DOC_FILES:
        if path.exists() and path not in yielded:
            yielded.add(path)
            yield path
    for root in CODE_ROOTS:
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if not path.is_file():
                continue
            if path in yielded:
                continue
            if path.suffix.lower() not in TEXT_EXTENSIONS:
                continue
            if ".egg-info/" in rel(path):
                continue
            yielded.add(path)
            yield path


def detect_lang(path: Path) -> str:
    suffix = path.suffix.lower()
    return {
        ".rs": "rust",
        ".py": "python",
        ".md": "markdown",
        ".txt": "text",
        ".ps1": "powershell",
        ".bat": "batch",
        ".toml": "toml",
        ".json": "json",
        ".html": "html",
        ".js": "javascript",
        ".css": "css",
    }.get(suffix, "text")


def detect_domain(path_str: str) -> str:
    if path_str.startswith("tools/viewer-core/src/"):
        return "rust_core"
    if path_str.startswith("desktop-nova/src-tauri/"):
        return "tauri_backend"
    if path_str.startswith("desktop-nova/src/"):
        return "desktop_nova"
    if path_str.startswith("scripts/"):
        return "tooling"
    if path_str.startswith("docs/") or path_str.endswith(".md"):
        return "docs"
    return "misc"


def extract_symbols(path: Path, text: str, lang: str) -> list[SymbolRecord]:
    symbols: list[SymbolRecord] = []
    current_impl: str | None = None
    for index, line in enumerate(text.splitlines(), start=1):
        if lang == "rust":
            impl_match = RUST_IMPL_RE.match(line)
            if impl_match:
                current_impl = impl_match.group("name")
            match = RUST_SYMBOL_RE.match(line)
            if not match:
                continue
            kind = match.group("kind")
            name = match.group("name")
            qualname = f"{current_impl}::{name}" if current_impl and kind == "fn" else name
            tags = infer_tags(path, text, extra_tokens=[name, qualname, kind])
            symbols.append(
                SymbolRecord(rel(path), lang, kind, name, index, line.strip(), qualname, tags)
            )
        elif lang == "python":
            match = PY_SYMBOL_RE.match(line)
            if not match:
                continue
            kind = match.group("kind")
            name = match.group("name")
            tags = infer_tags(path, text, extra_tokens=[name, kind])
            symbols.append(
                SymbolRecord(rel(path), lang, kind, name, index, line.strip(), name, tags)
            )
    return symbols


def infer_tags(path: Path, text: str, extra_tokens: list[str] | None = None) -> list[str]:
    tokens = set(extra_tokens or [])
    path_str = rel(path)
    lowered = f"{path_str}\n{text[:12000]}".lower()
    for hint_path, tags in MODULE_ROLE_HINTS.items():
        if path_str == hint_path:
            tokens.update(tags)
    for family in (
        "chest",
        "hopper",
        "piston",
        "moving_piston",
        "rail",
        "water",
        "barrel",
        "repeater",
        "glass",
        "redstone",
        "sign",
        "cauldron",
        "composter",
    ):
        if family in lowered:
            tokens.add(family)
    if "typed_model" in lowered or "typed model" in lowered:
        tokens.add("typed_model")
    if "culling" in lowered or "cullface" in lowered:
        tokens.add("culling")
    if "palette" in lowered:
        tokens.add("palette")
    if "atlas" in lowered:
        tokens.add("atlas")
    if "preview" in lowered:
        tokens.add("preview")
    if "litematic" in lowered or "nbt" in lowered:
        tokens.add("litematic")
    return sorted(token for token in tokens if token and len(token) < 64)


def doc_outline(text: str) -> list[str]:
    outline: list[str] = []
    for line in text.splitlines():
        match = HEADING_RE.match(line.strip())
        if match:
            outline.append(match.group(2).strip())
    return outline


def make_doc_passages(path: Path, text: str) -> list[tuple[str, int, int, str]]:
    lines = text.splitlines()
    passages: list[tuple[str, int, int, str]] = []
    start = 1
    bucket = 0
    chunk: list[str] = []
    for index, line in enumerate(lines, start=1):
        if HEADING_RE.match(line.strip()) and chunk:
            passages.append(("heading", start, index - 1, "\n".join(chunk).strip()))
            bucket += 1
            chunk = []
            start = index
        chunk.append(line)
        if len(chunk) >= 48:
            passages.append(("window", start, index, "\n".join(chunk).strip()))
            bucket += 1
            chunk = []
            start = index + 1
    if chunk:
        passages.append(("tail", start, len(lines), "\n".join(chunk).strip()))
    return [(kind, s, e, content) for kind, s, e, content in passages if content]


def make_code_passages(path: Path, text: str, symbols: list[SymbolRecord]) -> list[tuple[str, int, int, str]]:
    lines = text.splitlines()
    passages: list[tuple[str, int, int, str]] = []
    for symbol in symbols:
        start = max(1, symbol.line_start - 3)
        end = min(len(lines), symbol.line_start + 20)
        content = "\n".join(lines[start - 1 : end]).strip()
        if content:
            passages.append((f"symbol:{symbol.name}", start, end, content))
    if not passages:
        for offset in range(0, len(lines), 80):
            start = offset + 1
            end = min(len(lines), offset + 80)
            content = "\n".join(lines[offset:end]).strip()
            if content:
                passages.append(("window", start, end, content))
    return passages


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        DROP TABLE IF EXISTS meta;
        DROP TABLE IF EXISTS docs;
        DROP TABLE IF EXISTS files;
        DROP TABLE IF EXISTS symbols;
        DROP TABLE IF EXISTS anchors;
        DROP TABLE IF EXISTS passages;
        DROP TABLE IF EXISTS symbol_fts;
        DROP TABLE IF EXISTS passage_fts;

        CREATE TABLE meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE docs (
            path TEXT PRIMARY KEY,
            title TEXT,
            category TEXT,
            sha1 TEXT,
            bytes INTEGER,
            mtime REAL,
            abstract TEXT,
            outline_json TEXT
        );

        CREATE TABLE files (
            path TEXT PRIMARY KEY,
            lang TEXT,
            domain TEXT,
            sha1 TEXT,
            bytes INTEGER,
            mtime REAL,
            top_symbols_json TEXT,
            tags_json TEXT
        );

        CREATE TABLE symbols (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            lang TEXT NOT NULL,
            kind TEXT NOT NULL,
            name TEXT NOT NULL,
            qualname TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            signature TEXT,
            tags_json TEXT
        );

        CREATE TABLE anchors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            anchor_type TEXT NOT NULL,
            anchor_key TEXT NOT NULL,
            line_no INTEGER NOT NULL,
            payload_json TEXT
        );

        CREATE TABLE passages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL,
            domain TEXT NOT NULL,
            bucket TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            content TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE symbol_fts USING fts5(
            path, name, qualname, signature, tags,
            content='',
            tokenize='unicode61'
        );

        CREATE VIRTUAL TABLE passage_fts USING fts5(
            path, bucket, content,
            content='',
            tokenize='unicode61'
        );
        """
    )


def insert_meta(conn: sqlite3.Connection) -> None:
    rows = [
        ("repo_root", str(REPO_ROOT)),
        ("db_path", str(DB_PATH)),
        ("builder", "scripts/build_assistant_index.py"),
        ("project_state_json", json.dumps(PROJECT_STATE, ensure_ascii=False)),
    ]
    conn.executemany("INSERT INTO meta(key, value) VALUES (?, ?)", rows)


def index_file(conn: sqlite3.Connection, path: Path) -> dict[str, int]:
    text = read_text(path)
    path_str = rel(path)
    lang = detect_lang(path)
    domain = detect_domain(path_str)
    sha1 = sha1_text(text)
    stat = path.stat()
    symbols = extract_symbols(path, text, lang)
    tags = infer_tags(path, text)
    top_symbols = [symbol.qualname for symbol in symbols[:20]]

    conn.execute(
        """
        INSERT INTO files(path, lang, domain, sha1, bytes, mtime, top_symbols_json, tags_json)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            path_str,
            lang,
            domain,
            sha1,
            stat.st_size,
            stat.st_mtime,
            json.dumps(top_symbols, ensure_ascii=False),
            json.dumps(tags, ensure_ascii=False),
        ),
    )

    if path in DOC_FILES:
        conn.execute(
            """
            INSERT INTO docs(path, title, category, sha1, bytes, mtime, abstract, outline_json)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                path_str,
                path.name,
                "seed_doc",
                sha1,
                stat.st_size,
                stat.st_mtime,
                DOC_ABSTRACTS.get(path.name, ""),
                json.dumps(doc_outline(text), ensure_ascii=False),
            ),
        )

    for symbol in symbols:
        cur = conn.execute(
            """
            INSERT INTO symbols(path, lang, kind, name, qualname, line_start, signature, tags_json)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                symbol.path,
                symbol.lang,
                symbol.kind,
                symbol.name,
                symbol.qualname,
                symbol.line_start,
                symbol.signature,
                json.dumps(symbol.tags, ensure_ascii=False),
            ),
        )
        conn.execute(
            "INSERT INTO symbol_fts(rowid, path, name, qualname, signature, tags) VALUES (?, ?, ?, ?, ?, ?)",
            (
                cur.lastrowid,
                symbol.path,
                symbol.name,
                symbol.qualname,
                symbol.signature,
                " ".join(symbol.tags),
            ),
        )

    for line_no, line in enumerate(text.splitlines(), start=1):
        for flag in sorted(set(ENV_FLAG_RE.findall(line))):
            conn.execute(
                """
                INSERT INTO anchors(path, anchor_type, anchor_key, line_no, payload_json)
                VALUES (?, 'env_flag', ?, ?, ?)
                """,
                (path_str, flag, line_no, json.dumps({"line": line.strip()}, ensure_ascii=False)),
            )
        for block_id in sorted(set(BLOCK_ID_RE.findall(line))):
            conn.execute(
                """
                INSERT INTO anchors(path, anchor_type, anchor_key, line_no, payload_json)
                VALUES (?, 'block_id', ?, ?, ?)
                """,
                (
                    path_str,
                    block_id,
                    line_no,
                    json.dumps({"line": line.strip()}, ensure_ascii=False),
                ),
            )

    passages = make_doc_passages(path, text) if domain == "docs" else make_code_passages(path, text, symbols)
    for bucket, line_start, line_end, content in passages:
        cur = conn.execute(
            """
            INSERT INTO passages(path, domain, bucket, line_start, line_end, content)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            (path_str, domain, bucket, line_start, line_end, content),
        )
        conn.execute(
            "INSERT INTO passage_fts(rowid, path, bucket, content) VALUES (?, ?, ?, ?)",
            (cur.lastrowid, path_str, bucket, content),
        )

    return {
        "files": 1,
        "docs": 1 if path in DOC_FILES else 0,
        "symbols": len(symbols),
        "passages": len(passages),
    }


def build_index() -> None:
    DB_DIR.mkdir(parents=True, exist_ok=True)
    if DB_PATH.exists():
        DB_PATH.unlink()
    wal_path = DB_PATH.with_suffix(".sqlite-wal")
    shm_path = DB_PATH.with_suffix(".sqlite-shm")
    if wal_path.exists():
        wal_path.unlink()
    if shm_path.exists():
        shm_path.unlink()

    conn = sqlite3.connect(DB_PATH)
    try:
        ensure_schema(conn)
        insert_meta(conn)
        totals = {"files": 0, "docs": 0, "symbols": 0, "passages": 0}
        for path in iter_text_files():
            stats = index_file(conn, path)
            for key, value in stats.items():
                totals[key] += value
        conn.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?, ?)",
            ("build_totals_json", json.dumps(totals, ensure_ascii=False)),
        )
        conn.commit()
        print(f"built: {DB_PATH}")
        print(json.dumps(totals, ensure_ascii=False))
    finally:
        conn.close()


if __name__ == "__main__":
    build_index()
