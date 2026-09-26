#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""GitHub PR 工具 (gh CLI 未安装的替代; 2026-09-26 版本丢失, 2026-09-27 按 HANDOFF 42 条实战补遗重建)。

安全纪律:
  * token 只从 Windows 凭据管理器读入内存 (CredReadW), **不落盘、不回显、不进命令行**;
  * token 经 curl `--config -` (stdin) 注入, 命令行里只有无敏感的参数;
  * PR body 等 JSON 写临时文件 (无敏感内容), 用后即删;
  * REST 一律 curl + 本机代理 (python urllib 走该代理 TLS 会断, 见 HANDOFF 42)。

用法:
  python work/_gh_pr.py api <METHOD> <path> [body_json]
  python work/_gh_pr.py create-pr <head_branch> "<title>" "<body>"
  python work/_gh_pr.py merge-pr <pr_number>
  python work/_gh_pr.py pr-info <pr_number>
  python work/_gh_pr.py delete-branch <branch_name>
  python work/_gh_pr.py status <commit_sha>        # CI 状态 (combined status + check runs)
"""
import ctypes
import ctypes.wintypes as wt
import json
import subprocess
import sys
import tempfile
import os

REPO = "118coder/DfoVibration"
PROXY = os.environ.get("GH_PROXY", "http://127.0.0.1:7897")


class CREDENTIAL(ctypes.Structure):
    _fields_ = [
        ("Flags", wt.DWORD),
        ("Type", wt.DWORD),
        ("TargetName", wt.LPCWSTR),
        ("Comment", wt.LPCWSTR),
        ("LastWritten", wt.FILETIME),
        ("CredentialBlobSize", wt.DWORD),
        ("CredentialBlob", ctypes.POINTER(ctypes.c_byte)),
        ("Persist", wt.DWORD),
        ("AttributeCount", wt.DWORD),
        ("Attributes", ctypes.c_void_p),
        ("TargetAlias", wt.LPCWSTR),
        ("UserName", wt.LPCWSTR),
    ]


def read_token() -> str:
    """按 HANDOFF 42 优先级读凭据管理器: UTF-8 blob (gho_) 优先, UTF-16 (x-access-token) 兜底。"""
    advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
    advapi32.CredReadW.argtypes = [wt.LPCWSTR, wt.DWORD, wt.DWORD,
                                   ctypes.POINTER(ctypes.POINTER(CREDENTIAL))]
    targets = [
        ("GitHub - https://api.github.com/118coder", "utf-8"),
        ("git:https://github.com", "utf-16"),
    ]
    for target, enc in targets:
        cred_ptr = ctypes.POINTER(CREDENTIAL)()
        if advapi32.CredReadW(target, 1, 0, ctypes.byref(cred_ptr)):
            c = cred_ptr.contents
            size = c.CredentialBlobSize
            blob = ctypes.string_at(c.CredentialBlob, size)
            ctypes.windll.advapi32.CredFree(cred_ptr)
            token = blob.decode(enc, errors="strict").strip()
            if token:
                return token
    raise SystemExit("!! 凭据管理器里找不到 GitHub token (目标均读取失败)")


def curl_json(method: str, path: str, body=None) -> tuple[int, dict | list | str]:
    token = read_token()
    cfg_lines = [
        'header = "Authorization: token %s"' % token,
        'header = "Accept: application/vnd.github+json"',
        'header = "X-GitHub-Api-Version: 2022-11-28"',
        'proxy = "%s"' % PROXY,
        'request = "%s"' % method,
    ]
    tmp = None
    if body is not None:
        tmp = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
        json.dump(body, tmp, ensure_ascii=False)
        tmp.close()
        cfg_lines.append('data = "@%s"' % tmp.name.replace("\\", "/"))
    cfg_lines.append('url = "https://api.github.com/repos/%s/%s"' % (REPO, path))
    cfg = "\n".join(cfg_lines) + "\n"

    cmd = ["curl", "-s", "-S", "--show-error", "--config", "-",
           "-w", "\\n%{http_code}"]
    p = subprocess.run(cmd, input=cfg.encode("utf-8"), capture_output=True)
    if tmp:
        os.unlink(tmp.name)
    out = p.stdout.decode("utf-8", errors="replace")
    if p.returncode != 0:
        raise SystemExit("!! curl 失败: " + p.stderr.decode("utf-8", errors="replace"))
    if "\n" not in out:
        raise SystemExit("!! curl 无响应: " + out[:400])
    body_text, _, code = out.rpartition("\n")
    try:
        parsed = json.loads(body_text) if body_text.strip() else ""
    except json.JSONDecodeError:
        parsed = body_text
    return int(code), parsed


def main():
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return
    cmd = args[0]
    if cmd == "api":
        code, data = curl_json(args[1], args[2], json.loads(args[3]) if len(args) > 3 else None)
        print(code)
        print(json.dumps(data, ensure_ascii=False, indent=2)[:4000])
    elif cmd == "create-pr":
        head, title, body = args[1], args[2], args[3]
        code, data = curl_json("POST", "pulls", {"title": title, "head": head, "base": "main", "body": body})
        print(code)
        if code in (201, 422):  # 422 = 已存在同名 PR, 也要打印 number
            print("PR #%s  %s" % (data.get("number"), data.get("html_url", "")))
        else:
            print(json.dumps(data, ensure_ascii=False)[:2000])
        sys.exit(0 if code == 201 else 1)
    elif cmd == "merge-pr":
        code, data = curl_json("PUT", "pulls/%s/merge" % args[1], {"merge_method": "merge"})
        print(code)
        print(json.dumps(data, ensure_ascii=False)[:1500])
        sys.exit(0 if code == 200 else 1)
    elif cmd == "pr-info":
        code, data = curl_json("GET", "pulls/%s" % args[1])
        print(code)
        print(json.dumps({k: data.get(k) for k in ("number", "state", "merged", "merge_commit_sha", "html_url", "title")},
                         ensure_ascii=False, indent=2))
    elif cmd == "delete-branch":
        code, _ = curl_json("DELETE", "git/refs/heads/%s" % args[1])
        print(code)
        sys.exit(0 if code == 204 else 1)
    elif cmd == "status":
        sha = args[1]
        code, data = curl_json("GET", "commits/%s/status" % sha)
        print(code)
        if code == 200:
            print("state:", data.get("state"))
            for st in data.get("statuses", []):
                print(" -", st.get("context"), "=", st.get("state"), st.get("target_url"))
        code2, runs = curl_json("GET", "commits/%s/check-runs" % sha)
        if code2 == 200:
            for r in runs.get("check_runs", []):
                print(" -", r.get("name"), "=", r.get("status"), "/", r.get("conclusion"), r.get("html_url"))
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
