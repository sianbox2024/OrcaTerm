#!/usr/bin/env bash
set -euo pipefail

# 构建项目图谱并自检：.gitignore 命中的路径一律不得进入图谱。
#
# 用法：
#   ./ci/graph.sh            全量重建图谱 + 自检
#   ./ci/graph.sh --update   增量更新图谱 + 自检（其余参数透传给 graphify-rs build）
#
# 环境变量：
#   GRAPHIFY_OUT   图谱输出目录，默认 Temp/graphify-rs-out
#
# 退出码：
#   0  图谱构建成功，且不含 .gitignore 命中的路径
#   1  构建失败，或有被忽略的路径会进入图谱
#
# 背景：graphify-rs 只读 .graphifyignore，不解析 .gitignore（见
# graphify-detect/src/ignore.rs 的 load_graphifyignore），故 .gitignore 命中的
# 路径必须在 .graphifyignore 显式表达。历史上 Ref-src/ 因漏配被扫入 2570 个
# 文件，把图谱稀释成 83% 第三方代码；本脚本把该回归钉在构建那一刻。

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${GRAPHIFY_OUT:-Temp/graphify-rs-out}"

if ! command -v graphify-rs >/dev/null 2>&1; then
    echo "✗ 未找到 graphify-rs，请先 cargo install graphify-rs" >&2
    exit 1
fi

echo "==> 构建项目图谱：$OUT_DIR"
graphify-rs build -p . -o "$OUT_DIR" --code-only "$@"

echo "==> 自检：确认图谱未混入 .gitignore 命中的路径"
python3 - "$OUT_DIR" << 'PY'
import fnmatch
import json
import os
import subprocess
import sys

IGNORE_FILE = ".graphifyignore"
GLOB_CHARS = "*?["

status = 0


def warn(msg):
    print(f"    ⚠ {msg}")


def bad(msg):
    print(f"✗ {msg}", file=sys.stderr)


def git(*args, stdin=None):
    return subprocess.run(["git", *args], input=stdin, capture_output=True, text=True)


def load_patterns():
    try:
        with open(IGNORE_FILE, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except FileNotFoundError:
        return []
    return [p for p in (l.strip() for l in lines) if p and not p.startswith("#")]


def covered_by(path, patterns):
    """按 graphify-detect/src/ignore.rs 的语义判断 path 是否被某条规则覆盖。"""
    segs = path.split("/")
    for pat in patterns:
        if any(c in pat for c in GLOB_CHARS):
            if fnmatch.fnmatch(path, pat):
                return pat
            if "/" not in pat and any(fnmatch.fnmatch(s, pat) for s in segs):
                return pat
            continue
        if pat == path or pat == path + "/**":
            return pat
        if "/" not in pat and pat in segs:
            return pat
    return None


out_dir = sys.argv[1]

if git("rev-parse", "--is-inside-work-tree").stdout.strip() != "true":
    warn("当前不是 git 工作区，跳过基于 .gitignore 的校验")
    sys.exit(0)

patterns = load_patterns()

# 检查 0：死规则 lint。ignore.rs 只对「不含 / 」的模式做逐段匹配，带尾斜杠的
# 模式只能匹配字面量本身，永远命中不了实际路径 —— 属于静默失效的假覆盖。
dead = [p for p in patterns if p.endswith("/")]
if dead:
    warn(f"{IGNORE_FILE} 中 {len(dead)} 条规则带尾斜杠，按 ignore.rs 语义永不命中：")
    for p in dead:
        print(f"        {p}   → 应改写为 {p.rstrip('/')}")
    print("")

# 检查 1：静态覆盖。现存且被 git 忽略的路径必须都有对应规则，否则其中的代码
# 文件会在下次扫描时进入图谱（这一步不依赖图谱产物，能提前发现新增的忽略目录）。
ls = git("ls-files", "--others", "--ignored", "--exclude-standard", "--directory")
if ls.returncode != 0:
    warn(f"无法枚举被 git 忽略的路径，跳过静态覆盖校验：{ls.stderr.strip()}")
else:
    uncovered = []
    for raw in ls.stdout.splitlines():
        path = raw.strip().rstrip("/")
        if path and not covered_by(path, patterns):
            uncovered.append(path)
    if uncovered:
        status = 1
        bad(f"{len(uncovered)} 个被 .gitignore 忽略的路径未被 {IGNORE_FILE} 覆盖：")
        for path in uncovered:
            print(f"        {path}/   → 建议加入：{path} 与 {path}/**", file=sys.stderr)
        print("", file=sys.stderr)
    else:
        print(f"    ✓ 静态覆盖：被忽略的现存路径全部已排除")

# 检查 2：实扫兜底。以图谱产出的 manifest 为准，逐文件反查 .gitignore。
manifest = os.path.join(out_dir, ".graphify_manifest.json")
if not os.path.isfile(manifest):
    status = 1
    bad(f"未找到 {manifest}，无法核验图谱内容")
else:
    with open(manifest, encoding="utf-8") as f:
        data = json.load(f)
    files = sorted(set(data.get("files", {})) | set(data.get("hashes", {})))
    if not files:
        warn("图谱为空，无可校验内容")
    else:
        ci = git("check-ignore", "-v", "--no-index", "--stdin", stdin="\n".join(files))
        if ci.returncode > 1:
            status = 1
            bad(f"git check-ignore 执行失败：{ci.stderr.strip()}")
        else:
            hits = [l for l in ci.stdout.splitlines() if l.strip()]
            if hits:
                status = 1
                bad(f"图谱 {len(files)} 个文件中有 {len(hits)} 个被 .gitignore 忽略：")
                for h in hits:
                    meta, _, pathname = h.partition("\t")
                    print(f"        {pathname}   （由 {meta} 排除）", file=sys.stderr)
                print(
                    f"        修正：把上述路径加入 {IGNORE_FILE} 后重跑本脚本",
                    file=sys.stderr,
                )
            else:
                print(f"    ✓ 实扫校验：图谱 {len(files)} 个文件均未被 .gitignore 命中")

sys.exit(status)
PY