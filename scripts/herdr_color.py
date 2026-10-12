#!/usr/bin/env python3
"""Set local pane color rules for the custom Herdr build (Python 3.11+, Unix)."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import time
import tomllib

PRESETS = {
    "orange": ("#080604", "#FF7A00"),
    "blue": ("#04070B", "#2FA8FF"),
    "purple": ("#08050B", "#9D5CFF"),
    "green": ("#040907", "#39FF6A"),
    "red": ("#0B0505", "#FF1E1E"),
    "teal": ("#040909", "#00E0B0"),
}
TEXT = "#FFC98A"
START = "# BEGIN herdr-color managed rules"
END = "# END herdr-color managed rules"


def color(value):
    if not re.fullmatch(r"#[0-9a-fA-F]{6}", value):
        raise ValueError("color must be #RRGGBB, for example '#07080B'")
    return value.upper()


def split_content(content):
    """Own only an explicitly marked tail; keep all other config bytes intact."""
    tomllib.loads(content)
    if START not in content and END not in content:
        return content, []
    if content.count(START) != 1 or content.count(END) != 1:
        raise ValueError("ambiguous herdr-color markers; config left unchanged")
    start, end = content.index(START), content.index(END)
    if (start and content[start - 1] != "\n") or end < start:
        raise ValueError("invalid managed block; config left unchanged")
    if content[end + len(END):].strip():
        raise ValueError("managed block must be at the end of config; config left unchanged")
    for line in content[start:end].splitlines():
        stripped = line.strip()
        if not stripped or stripped in (START, "[[ui.pane_colors.rules]]"):
            continue
        if not re.fullmatch(r"(?:pane|foreground|background|border|theme)\s*=\s*(?:\"(?:[^\"\\]|\\.)*\"|'[^']*')", stripped):
            raise ValueError("managed block has manual comments or formatting; move annotations outside it before editing")
    base = content[:start]
    managed = tomllib.loads(content[start:end]).get("ui", {}).get("pane_colors", {})
    if set(managed) - {"rules"}:
        raise ValueError("managed block contains unrelated keys; config left unchanged")
    rules = managed.get("rules", [])
    if not isinstance(rules, list) or any(
        not isinstance(rule, dict)
        or not isinstance(rule.get("pane"), str)
        or set(rule) - {"pane", "foreground", "background", "border", "theme"}
        for rule in rules
    ):
        raise ValueError("invalid managed rules; config left unchanged")
    # Detect unrelated sections accidentally inserted in the managed block.
    block = tomllib.loads(content[start:end])
    if set(block) - {"ui"} or set(block.get("ui", {})) - {"pane_colors"}:
        raise ValueError("managed block contains unrelated sections; config left unchanged")
    return base, rules


def updated_content(content, target, scheme, foreground=None, border=None):
    base, rules = split_content(content)
    rules = [rule for rule in rules if rule["pane"] != target]
    if scheme != "reset":
        background, frame = PRESETS.get(scheme, (None, None))
        background = background or color(scheme)
        rules.append({
            "pane": target,
            "foreground": color(foreground) if foreground else TEXT,
            "background": background,
            "border": color(border) if border else frame or PRESETS["orange"][1],
        })
    result = base
    if rules:
        result += ("" if result.endswith("\n") else "\n") + START + "\n"
        for rule in rules:
            result += "\n[[ui.pane_colors.rules]]\n"
            for key, value in rule.items():
                result += f"{key} = {json.dumps(value, ensure_ascii=False)}\n"
        result += END + "\n"
    tomllib.loads(result)  # Reject unsupported TOML representations before any write.
    return result


def save(path, target, scheme, foreground=None, border=None):
    path = path.expanduser().resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path.with_name(path.name + ".color.lock"), "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        before = path.read_bytes() if path.exists() else b""
        after = updated_content(before.decode("utf-8-sig"), target, scheme, foreground, border).encode()
        if before.startswith(b"\xef\xbb\xbf"):
            after = b"\xef\xbb\xbf" + after
        if after == before:
            return None
        mode = stat.S_IMODE(path.stat().st_mode) if path.exists() else 0o600
        backup = None
        if path.exists():
            folder = path.parent / "pane-color-backups"
            folder.mkdir(mode=0o700, exist_ok=True)
            backup = folder / f"{path.name}.{time.time_ns()}.bak"
            with open(backup, "xb") as stream:
                os.chmod(backup, 0o600)
                stream.write(before)
        fd, name = tempfile.mkstemp(prefix=".herdr-color-", dir=path.parent)
        try:
            with os.fdopen(fd, "wb") as stream:
                os.fchmod(stream.fileno(), mode)
                stream.write(after)
                stream.flush()
                os.fsync(stream.fileno())
            if (path.read_bytes() if path.exists() else b"") != before:
                raise ValueError("config changed while saving; retry the command")
            os.replace(name, path)
        finally:
            if os.path.exists(name):
                os.unlink(name)
        return backup


def request(args):
    result = subprocess.run(["herdr", *args], capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError(result.stderr.strip() or result.stdout.strip() or "Herdr command failed")
    response = json.loads(result.stdout)
    if "error" in response:
        raise ValueError(response["error"].get("message", "Herdr command failed"))
    return response["result"]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scheme", nargs="?", help="orange, blue, purple, green, red, teal, #RRGGBB, or reset")
    targets = parser.add_mutually_exclusive_group()
    targets.add_argument("--current", action="store_true", help="resolve the calling pane through Herdr")
    targets.add_argument("--pane", help="exact pane ID from herdr pane list")
    targets.add_argument("--name", help="persistent manual pane label or agent name")
    parser.add_argument("--foreground", type=color)
    parser.add_argument("--border", type=color)
    parser.add_argument("--list", action="store_true", help="show available presets")
    parser.add_argument("--no-reload", action="store_true", help="save only; reload the client manually")
    config = os.environ.get("HERDR_CONFIG_PATH") or str(
        Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "herdr" / "config.toml"
    )
    parser.add_argument("--config", type=Path, default=Path(config), help="local client config path")
    args = parser.parse_args(argv)
    if args.config.expanduser().resolve() != Path(config).expanduser().resolve():
        args.no_reload = True
    if args.list:
        for name, (bg, frame) in PRESETS.items():
            print(f"{name:7} background={bg} border={frame} foreground={TEXT}")
        return 0
    if args.scheme is None or not (args.current or args.pane or args.name):
        parser.error("choose --current, --pane ID, or --name LABEL, and a color")
    try:
        if args.scheme not in PRESETS and args.scheme != "reset":
            color(args.scheme)
        if args.scheme == "reset" and (args.foreground or args.border):
            raise ValueError("reset cannot be combined with color overrides")
        if args.current:
            caller = os.environ.get("HERDR_PANE_ID", "").strip()
            if os.environ.get("HERDR_ENV") != "1" or not caller or any(ord(char) < 32 for char in caller):
                raise ValueError("--current requires a Herdr pane; use --pane ID from herdr pane list")
            target = request(["pane", "current", "--pane", caller])["pane"]["pane_id"]
        elif args.pane:
            # Validate IDs, including stale/closed panes, before saving a rule.
            target = request(["pane", "get", args.pane])["pane"]["pane_id"]
        else:
            target = args.name
        if not target or any(ord(char) < 32 for char in target):
            raise ValueError("pane selector must be nonempty and contain no control characters")
        backup = save(args.config, target, args.scheme, args.foreground, args.border)
        print(f"Saved {args.scheme} for {target} in {args.config}")
        if backup:
            print(f"Backup: {backup}")
        if not args.no_reload:
            try:
                response = request(["server", "reload-config"])
                if response.get("diagnostics"):
                    print("Server reports config warnings; reload your local client with Ctrl+B, Shift+R.")
                else:
                    print("Config reload requested for attached clients.")
            except (ValueError, OSError, subprocess.TimeoutExpired, KeyError) as error:
                print(f"Saved; automatic reload unavailable ({error}). Use Ctrl+B, Shift+R.", file=sys.stderr)
        else:
            print("Reload your local client with Ctrl+B, Shift+R.")
        return 0
    except (ValueError, OSError, subprocess.TimeoutExpired, KeyError) as error:
        print(f"herdr-color: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
