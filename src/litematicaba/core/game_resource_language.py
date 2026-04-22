"""游戏资源语言：来源、下载、裁切与已装载索引。"""

from __future__ import annotations

import json
import re
import subprocess
import urllib.request
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from litematicaba.core.config import project_root, user_data_dir

LANG_SOURCE_BUILTIN = "builtin"
LANG_SOURCE_GITHUB_INVENTIVETALENT = "github/InventivetalentDev"

_LANG_ALLOWED_PREFIXES = (
    "block.",
    "effect.",
    "enchantment.",
    "entity.",
    "item.",
)
_GH_API = "https://api.github.com/repos/InventivetalentDev/minecraft-assets"
_GH_RAW = "https://raw.githubusercontent.com/InventivetalentDev/minecraft-assets"


@dataclass
class InstalledLanguage:
    id: str
    language: str
    branch: str
    source: str
    file_relpath: str
    active: bool = False
    installed_at: str = ""


def _language_root() -> Path:
    return user_data_dir() / "minecraft-assets" / "language"


def _initial_dir() -> Path:
    return _language_root() / "initial"


def _github_dir() -> Path:
    return _language_root() / "github" / "InventivetalentDev"

# pack-in 语言位置
def _legacy_dir() -> Path:
    return user_data_dir() / "legacy"


def _bundled_lang_file(name: str) -> Path:
    # 与 legacy_statistics 等一致：project_root() 在 frozen 下为 _MEIPASS。
    return project_root() / "pack-in" / "lang" / name


def _bundled_legacy_file(name: str) -> Path:
    return project_root() / "pack-in" / "legacy" / name


# 初始化解包语言
def ensure_initial_language_seeded() -> None:
    # 1. 游戏语言 zh_cn.json
    init_dir = _initial_dir()
    init_dir.mkdir(parents=True, exist_ok=True)
    dst_lang = init_dir / "zh_cn.json"
    if not dst_lang.is_file():
        src_lang = _bundled_lang_file("zh_cn.json")
        if src_lang.is_file():
            try:
                dst_lang.write_bytes(src_lang.read_bytes())
            except OSError:
                pass

    # 2. 遗留分类 category.json -> data/legacy
    leg_dir = _legacy_dir()
    leg_dir.mkdir(parents=True, exist_ok=True)
    dst_cat = leg_dir / "category.json"
    if not dst_cat.is_file():
        src_cat = _bundled_legacy_file("category.json")
        if src_cat.is_file():
            try:
                dst_cat.write_bytes(src_cat.read_bytes())
            except OSError:
                pass


def _index_path() -> Path:
    return _language_root() / "installed.json"


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _fetch_json(url: str) -> Any:
    req = urllib.request.Request(url, headers={"User-Agent": "LitematicaBA-lang-manager/1.0"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read().decode("utf-8"))


def _fetch_text(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "LitematicaBA-lang-manager/1.0"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.read().decode("utf-8")


def _sanitize_branch_or_lang(s: str) -> str:
    return re.sub(r"[^a-zA-Z0-9._-]+", "_", (s or "").strip())


def _branch_allowed(name: str) -> bool:
    s = (name or "").strip()
    if not s:
        return False
    if re.fullmatch(r"1\.\d+(?:\.\d+)?", s):
        return True
    if re.fullmatch(r"\d{2}\.\d+(?:\.\d+)?", s):
        return True
    return False


def _branch_version_key(name: str) -> tuple[int, ...]:
    parts = (name or "").strip().split(".")
    nums: list[int] = []
    for p in parts:
        try:
            nums.append(int(p))
        except ValueError:
            nums.append(-1)
    # 统一到三段，避免 1.20 和 1.20.1 比较不稳定
    while len(nums) < 3:
        nums.append(-1)
    return tuple(nums[:3])


def _lang_code_allowed(code: str) -> bool:
    s = (code or "").strip().lower()
    # 常见 Minecraft 语言码：
    # - 无地区码：lzh
    # - 含地区码：zh_cn / en_us
    # - 含脚本后缀：zh_hans_cn
    return bool(re.fullmatch(r"[a-z]{2,3}(?:_[a-z0-9]{2,8}){0,2}", s))


def fetch_github_branches() -> list[str]:
    out: list[str] = []
    try:
        page = 1
        while True:
            url = f"{_GH_API}/branches?per_page=100&page={page}"
            data = _fetch_json(url)
            if not isinstance(data, list) or not data:
                break
            for row in data:
                if not isinstance(row, dict):
                    continue
                name = row.get("name")
                if isinstance(name, str) and _branch_allowed(name):
                    out.append(name)
            if len(data) < 100:
                break
            page += 1
    except Exception:
        # API 可能触发匿名额度限制，优先回退 git ls-remote。
        try:
            proc = subprocess.run(
                ["git", "ls-remote", "--heads", "https://github.com/InventivetalentDev/minecraft-assets.git"],
                check=False,
                capture_output=True,
                text=True,
                timeout=30,
            )
            if proc.returncode == 0:
                for line in proc.stdout.splitlines():
                    m = re.search(r"refs/heads/(.+)$", line.strip())
                    if not m:
                        continue
                    name = m.group(1)
                    if _branch_allowed(name):
                        out.append(name)
        except Exception:
            pass
        if not out:
            html = _fetch_text("https://github.com/InventivetalentDev/minecraft-assets/branches")
            for name in re.findall(r"/InventivetalentDev/minecraft-assets/tree/([^\"]+)", html):
                if _branch_allowed(name):
                    out.append(name)
    return sorted(set(out), key=_branch_version_key, reverse=True)


def _parse_lang_list_payload(raw: Any) -> list[str]:
    out: list[str] = []
    if isinstance(raw, list):
        for item in raw:
            if isinstance(item, str) and _lang_code_allowed(item):
                out.append(item.lower())
                continue
            if isinstance(item, dict):
                for key in ("code", "lang", "id", "name"):
                    v = item.get(key)
                    if isinstance(v, str) and _lang_code_allowed(v):
                        out.append(v.lower())
                        break
    elif isinstance(raw, dict):
        for k, v in raw.items():
            if isinstance(k, str) and _lang_code_allowed(k):
                out.append(k.lower())
            if isinstance(v, str) and _lang_code_allowed(v):
                out.append(v.lower())
            if isinstance(v, dict):
                for key in ("code", "lang", "id", "name"):
                    x = v.get(key)
                    if isinstance(x, str) and _lang_code_allowed(x):
                        out.append(x.lower())
                        break
    return sorted(set(out))


def fetch_github_languages(branch: str) -> list[str]:
    b = (branch or "").strip()
    if not b:
        return []
    # 优先使用分支内语言列表文件。
    try:
        list_url = f"{_GH_RAW}/{b}/assets/minecraft/lang/_list.json"
        list_payload = json.loads(_fetch_text(list_url))
        parsed = _parse_lang_list_payload(list_payload)
        if parsed:
            return parsed
    except Exception:
        pass
    out: list[str] = []
    try:
        url = f"{_GH_API}/contents/assets/minecraft/lang?ref={b}"
        data = _fetch_json(url)
        if not isinstance(data, list):
            return out
        for row in data:
            if not isinstance(row, dict):
                continue
            typ = row.get("type")
            name = row.get("name")
            if typ != "file" or not isinstance(name, str):
                continue
            if name.lower().endswith(".json"):
                code = name[:-5].lower()
                if _lang_code_allowed(code):
                    out.append(code)
    except Exception:
        html = _fetch_text(
            f"https://github.com/InventivetalentDev/minecraft-assets/tree/{b}/assets/minecraft/lang"
        )
        for name in re.findall(r"([a-z0-9_]+)\.json", html, flags=re.IGNORECASE):
            code = name.lower()
            if _lang_code_allowed(code):
                out.append(code)
    return sorted(set(out))


def _filter_translations(raw: dict[str, Any]) -> dict[str, str]:
    out: dict[str, str] = {}
    for k, v in raw.items():
        if not isinstance(k, str) or not isinstance(v, str):
            continue
        if any(k.startswith(p) for p in _LANG_ALLOWED_PREFIXES):
            out[k] = v
    return out


def download_github_language(*, branch: str, language: str) -> InstalledLanguage:
    b = (branch or "").strip()
    lang = (language or "").strip().lower()
    if not b or not lang:
        raise ValueError("分支或语言为空")
    url = f"{_GH_RAW}/{b}/assets/minecraft/lang/{lang}.json"
    payload = json.loads(_fetch_text(url))
    if not isinstance(payload, dict):
        raise ValueError("语言文件格式错误")
    trimmed = _filter_translations(payload)
    root = _language_root()
    source_dir = _github_dir() / _sanitize_branch_or_lang(b)
    source_dir.mkdir(parents=True, exist_ok=True)
    file_name = f"{_sanitize_branch_or_lang(lang)}.json"
    rel = f"github/InventivetalentDev/{_sanitize_branch_or_lang(b)}/{file_name}"
    dst = root / rel
    dst.write_text(json.dumps(trimmed, ensure_ascii=False, indent=2), encoding="utf-8")
    return InstalledLanguage(
        id=f"{LANG_SOURCE_GITHUB_INVENTIVETALENT}:{b}:{lang}",
        language=lang,
        branch=b,
        source=LANG_SOURCE_GITHUB_INVENTIVETALENT,
        file_relpath=rel,
        active=False,
        installed_at=_now_iso(),
    )


def load_installed_languages() -> list[InstalledLanguage]:
    ensure_initial_language_seeded()
    path = _index_path()
    if not path.is_file():
        return []
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return []
    if not isinstance(raw, list):
        return []
    out: list[InstalledLanguage] = []
    for row in raw:
        if not isinstance(row, dict):
            continue
        try:
            item = InstalledLanguage(
                id=str(row.get("id", "")),
                language=str(row.get("language", "")),
                branch=str(row.get("branch", "")),
                source=str(row.get("source", "")),
                file_relpath=str(row.get("file_relpath", "")),
                active=bool(row.get("active", False)),
                installed_at=str(row.get("installed_at", "")),
            )
        except Exception:
            continue
        if not item.id or not item.file_relpath:
            continue
        # 历史错误数据兼容：内建项不应出现在 installed 索引中。
        if item.source == LANG_SOURCE_BUILTIN or item.id == LANG_SOURCE_BUILTIN:
            continue
        out.append(item)
    return out


def save_installed_languages(items: list[InstalledLanguage]) -> None:
    root = _language_root()
    root.mkdir(parents=True, exist_ok=True)
    data = [asdict(i) for i in items]
    _index_path().write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")


def upsert_installed_language(entry: InstalledLanguage) -> list[InstalledLanguage]:
    items = load_installed_languages()
    replaced = False
    for idx, old in enumerate(items):
        if old.id == entry.id:
            items[idx] = entry
            replaced = True
            break
    if not replaced:
        items.append(entry)
    save_installed_languages(items)
    return items


def set_active_language(installed_id: str) -> list[InstalledLanguage]:
    items = load_installed_languages()
    for i in range(len(items)):
        items[i].active = items[i].id == installed_id
    save_installed_languages(items)
    return items


def delete_installed_language(installed_id: str) -> list[InstalledLanguage]:
    items = load_installed_languages()
    kept: list[InstalledLanguage] = []
    for item in items:
        if item.id != installed_id:
            kept.append(item)
            continue
        p = _language_root() / item.file_relpath
        try:
            p.unlink(missing_ok=True)
        except OSError:
            pass
    save_installed_languages(kept)
    return kept


def load_runtime_language_map() -> dict[str, str]:
    ensure_initial_language_seeded()
    items = load_installed_languages()
    for item in items:
        if not item.active:
            continue
        p = _language_root() / item.file_relpath
        try:
            raw = json.loads(p.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        if isinstance(raw, dict):
            return {str(k): str(v) for k, v in raw.items() if isinstance(k, str) and isinstance(v, str)}
    # 内建回退：首次运行已复制到 data/.../language/initial。
    builtin = _initial_dir() / "zh_cn.json"
    try:
        raw = json.loads(builtin.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    if not isinstance(raw, dict):
        return {}
    out: dict[str, str] = {}
    for k, v in raw.items():
        if isinstance(k, str) and isinstance(v, str):
            out[k] = v
    return out
