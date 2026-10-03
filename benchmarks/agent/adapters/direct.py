#!/usr/bin/env python3
"""Built-in agent loop: run one benchmark trial against a model API with no agent harness around it.

Honors the BENCH_* contract (docs/agent-benchmark.md). BENCH_MODEL is `anthropic/<model>` (ANTHROPIC_API_KEY, optional
ANTHROPIC_BASE_URL) or `openai/<model>` (OPENAI_API_KEY, optional OPENAI_BASE_URL, so any OpenAI-compatible endpoint works).
The model gets one `bash` tool that runs in the workspace on the shimmed PATH, plus `fetch` only when knowledge is `web`.
Under tool level `mcp` it also registers `BENCH_MCP_CMD` (`wright serve --transport mcp`) and exposes the server's tools
natively; the `wright` CLI is then not on PATH.
The system prompt lists the installed skills by name and description, and the model reads their files itself.
The loop, its limits, and the tool set are fixed here so every model faces the same protocol; the recorded harness commit identifies them.
It does not sandbox the network: pair it with the harness --canary-cmd. Exit 75 marks a provider or infrastructure failure.
"""

from __future__ import annotations

import json
import os
import re
import selectors
import shlex
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

from common import INFRA_EXIT

MAX_TURNS = 60
COMMAND_SECONDS = 120
OUTPUT_CHARS = 20_000
MAX_OUTPUT_TOKENS = 16_000
RETRIES = 3
SYSTEM = "You are a coding agent working in the current directory. Use the bash tool to inspect and edit files. Finish with a short summary of what you did."
BASH = {"name": "bash", "description": "Run a shell command in the workspace and return its output.", "schema": {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}}
FETCH = {"name": "fetch", "description": "HTTP GET a URL and return the start of the body.", "schema": {"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}}


class ProviderError(Exception):
    def __init__(self, message: str, transient: bool):
        super().__init__(message)
        self.transient = transient


def post(url: str, headers: dict, body: dict) -> dict:
    request = urllib.request.Request(url, json.dumps(body).encode(), {"content-type": "application/json", **headers})
    for attempt in range(RETRIES + 1):
        try:
            with urllib.request.urlopen(request, timeout=600) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            text = error.read().decode(errors="replace")[:500]
            if error.code in (408, 429) or error.code >= 500:
                if attempt < RETRIES:
                    time.sleep(2 ** attempt * 5)
                    continue
                raise ProviderError(f"HTTP {error.code}: {text}", True)
            raise ProviderError(f"HTTP {error.code}: {text}", False)
        except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
            if attempt < RETRIES:
                time.sleep(2 ** attempt * 5)
                continue
            raise ProviderError(str(error), True)
    raise AssertionError


def usage_row(inp: int, out: int, cache_read: int, cache_write: int, reasoning: int | None) -> dict:
    return {"t": time.time(), "input": inp, "output": out, "cache_read": cache_read, "cache_write": cache_write, "reasoning": reasoning, "context": inp + cache_read + cache_write, "context_limit": None}


class Anthropic:
    def __init__(self, model: str, tools: list[dict], system: str):
        self.model, self.system = model, system
        self.url = os.environ.get("ANTHROPIC_BASE_URL", "https://api.anthropic.com").rstrip("/") + "/v1/messages"
        self.headers = {"x-api-key": os.environ["ANTHROPIC_API_KEY"], "anthropic-version": "2023-06-01"}
        self.tools = [{"name": t["name"], "description": t["description"], "input_schema": t["schema"]} for t in tools]
        self.messages: list[dict] = []

    def user(self, text: str) -> None:
        self.messages.append({"role": "user", "content": text})

    def results(self, results: list[tuple[str, str]]) -> None:
        self.messages.append({"role": "user", "content": [{"type": "tool_result", "tool_use_id": i, "content": out} for i, out in results]})

    def step(self) -> tuple[str, list[tuple[str, str, dict]], dict]:
        data = post(self.url, self.headers, {"model": self.model, "max_tokens": MAX_OUTPUT_TOKENS, "system": self.system, "tools": self.tools, "messages": self.messages})
        try:
            content = data["content"]
            text = "".join(b.get("text", "") for b in content if b["type"] == "text")
            calls = [(b["id"], b["name"], b["input"]) for b in content if b["type"] == "tool_use"]
        except (KeyError, IndexError, TypeError, AttributeError) as error:
            raise ProviderError(f"malformed response: {error}", False) from error
        self.messages.append({"role": "assistant", "content": content})
        u = data.get("usage") or {}
        return text, calls, usage_row(u.get("input_tokens") or 0, u.get("output_tokens") or 0, u.get("cache_read_input_tokens") or 0, u.get("cache_creation_input_tokens") or 0, None)


class OpenAI:
    def __init__(self, model: str, tools: list[dict], system: str, effort: str | None):
        self.model, self.effort = model, effort
        self.url = os.environ.get("OPENAI_BASE_URL", "https://api.openai.com/v1").rstrip("/") + "/chat/completions"
        self.headers = {"authorization": f"Bearer {os.environ['OPENAI_API_KEY']}"}
        self.tools = [{"type": "function", "function": {"name": t["name"], "description": t["description"], "parameters": t["schema"]}} for t in tools]
        self.messages: list[dict] = [{"role": "system", "content": system}]

    def user(self, text: str) -> None:
        self.messages.append({"role": "user", "content": text})

    def results(self, results: list[tuple[str, str]]) -> None:
        self.messages += [{"role": "tool", "tool_call_id": i, "content": out} for i, out in results]

    def step(self) -> tuple[str, list[tuple[str, str, dict]], dict]:
        body = {"model": self.model, "tools": self.tools, "messages": self.messages, **({"reasoning_effort": self.effort} if self.effort else {})}
        data = post(self.url, self.headers, body)
        try:
            message = data["choices"][0]["message"]
            calls = [(c["id"], c["function"]["name"], json.loads(c["function"]["arguments"] or "{}")) for c in message.get("tool_calls") or []]
        except (KeyError, IndexError, TypeError, AttributeError, json.JSONDecodeError) as error:
            raise ProviderError(f"malformed response: {error}", False) from error
        self.messages.append(message)
        u = data.get("usage") or {}
        cached = (u.get("prompt_tokens_details") or {}).get("cached_tokens") or 0
        reasoning = (u.get("completion_tokens_details") or {}).get("reasoning_tokens") or 0
        return message.get("content") or "", calls, usage_row((u.get("prompt_tokens") or 0) - cached, (u.get("completion_tokens") or 0) - reasoning, cached, 0, reasoning)


class Mcp:
    """A `wright serve --transport mcp` server over stdio (ADR-0020): line-delimited JSON-RPC, one request at a time."""

    def __init__(self, command: str, env: dict):
        self.proc = subprocess.Popen(["/bin/sh", "-c", command], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
        self.next_id = 0

    def request(self, method: str, params: dict | None = None) -> dict:
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": self.next_id, "method": method}
        if params is not None:
            message["params"] = params
        try:
            self.proc.stdin.write(json.dumps(message) + "\n")
            self.proc.stdin.flush()
            for line in self._lines():
                reply = json.loads(line)
                if isinstance(reply, dict) and reply.get("id") == self.next_id:
                    return reply
        except (BrokenPipeError, json.JSONDecodeError, OSError):
            pass
        return {"error": {"code": -32000, "message": "the MCP server closed or timed out"}}

    def _lines(self):
        """Reply lines, bounded to COMMAND_SECONDS where the OS can poll a pipe; a hung server then reads as a tool error."""
        if os.name != "posix":
            yield from self.proc.stdout
            return
        deadline = time.monotonic() + COMMAND_SECONDS
        selector = selectors.DefaultSelector()
        with selector:
            selector.register(self.proc.stdout, selectors.EVENT_READ)
            while selector.select(max(0.0, deadline - time.monotonic())):
                line = self.proc.stdout.readline()
                if not line:
                    return
                yield line

    def notify(self, method: str) -> None:
        try:
            self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError):
            pass

    def close(self) -> None:
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()
        self.proc.stdout.close()


MCP: Mcp | None = None


def mcp_tools(command: str, env: dict) -> list[dict]:
    """Register the MCP server and return its tools in this loop's {name, description, schema} form."""
    global MCP
    MCP = Mcp(command, env)
    try:
        initialized = MCP.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "wright-agent-bench", "version": "1"}})
        if "error" in initialized:
            raise ProviderError(f"MCP initialize failed: {initialized['error'].get('message')}", False)
        MCP.notify("notifications/initialized")
        listed = (MCP.request("tools/list").get("result") or {}).get("tools") or []
        if not listed:
            raise ProviderError("MCP tools/list returned no tools", False)
    except ProviderError:
        MCP.close()
        MCP = None
        raise
    return [{"name": tool["name"], "description": tool.get("description") or "", "schema": tool.get("inputSchema") or {"type": "object"}} for tool in listed]


def run_tool(name: str, args: dict, env: dict) -> str:
    try:
        if name == "bash":
            done = subprocess.run(["bash", "--noprofile", "--norc", "-c", args["command"]], capture_output=True, text=True, timeout=COMMAND_SECONDS, env=env, errors="replace")
            out = done.stdout + done.stderr + (f"\n[exit {done.returncode}]" if done.returncode else "")
        elif name == "fetch":
            with urllib.request.urlopen(args["url"], timeout=60) as response:
                out = response.read(OUTPUT_CHARS * 2).decode(errors="replace")
        elif MCP:
            reply = MCP.request("tools/call", {"name": name, "arguments": args})
            if "error" in reply:
                return f"[mcp error {reply['error'].get('code')}: {reply['error'].get('message')}]"
            result = reply.get("result") or {}
            out = "\n".join(block.get("text", "") for block in result.get("content") or [] if isinstance(block, dict) and block.get("type") == "text")
            if result.get("isError"):
                out = f"[isError] {out}"
        else:
            return f"unknown tool {name}"
    except subprocess.TimeoutExpired:
        return f"[timed out after {COMMAND_SECONDS}s]"
    except Exception as error:
        return f"[{type(error).__name__}: {error}]"
    return out if len(out) <= OUTPUT_CHARS else out[:OUTPUT_CHARS] + f"\n[truncated {len(out) - OUTPUT_CHARS} characters]"


def skill_listing(skills: list[Path]) -> tuple[str, list[str]]:
    lines, names = [], []
    for skill in skills:
        target = Path(".agents/skills") / skill.name
        shutil.copytree(skill, target)
        text = (target / "SKILL.md").read_text()
        name = re.search(r"^name:\s*(\S+)", text, re.M)  # same first-word rule as the harness's skill_name check
        description = re.search(r"^description:\s*(.+)$", text, re.M)
        names.append(name.group(1).strip() if name else skill.name)
        lines.append(f"- {names[-1]}: {description.group(1).strip() if description else ''} (read {target}/SKILL.md when relevant)")
    return ("\n\nSkills available:\n" + "\n".join(lines)) if lines else "", names


def main() -> int:
    env = os.environ
    provider, _, model = env["BENCH_MODEL"].partition("/")
    if provider not in ("anthropic", "openai") or not model:
        print("BENCH_MODEL must be anthropic/<model> or openai/<model>", file=sys.stderr)
        return 2
    web = env["BENCH_KNOWLEDGE"] == "web"
    tools = [BASH] + ([FETCH] if web else [])
    child_env = {k: v for k, v in env.items() if k not in ("BENCH_HOST_PATH", "ANTHROPIC_API_KEY", "OPENAI_API_KEY")}
    global MCP
    MCP = None
    if env.get("BENCH_MCP_CMD"):
        try:
            tools += mcp_tools(env["BENCH_MCP_CMD"], child_env)
        except ProviderError as error:
            print(f"mcp setup: {error}", file=sys.stderr)
            return 1
    listing, loaded = skill_listing([Path(p) for p in env.get("BENCH_SKILL_DIRS", "").split(os.pathsep) if p])
    effort = env.get("BENCH_THINKING") if provider == "openai" else None
    system = SYSTEM + listing
    chat = Anthropic(model, tools, system) if provider == "anthropic" else OpenAI(model, tools, system, effort)
    Path(env["BENCH_AGENT_INFO"]).write_text(json.dumps({
        "agent": "direct", "model": env["BENCH_MODEL"], "effort": effort, "tools": [t["name"] for t in tools],
        "toolLevel": env.get("BENCH_TOOL_LEVEL", "bin"),
        "protocol": {"maxTurns": MAX_TURNS, "commandSeconds": COMMAND_SECONDS, "outputChars": OUTPUT_CHARS, "maxOutputTokens": MAX_OUTPUT_TOKENS}}, indent=2))
    Path(env["BENCH_CONTEXT"]).write_text(json.dumps({"loaded": loaded}))
    chat.user(sys.stdin.read())
    final, code = "", 0
    try:
        with open(env["BENCH_USAGE"], "w", buffering=1) as usage, open(env["BENCH_TRANSCRIPT"], "w", buffering=1) as transcript:
            transcript.write(json.dumps({"t": time.time(), "type": "system", "text": system}) + "\n")
            for _ in range(MAX_TURNS):
                try:
                    text, calls, row = chat.step()
                except ProviderError as error:
                    print(f"provider error: {error}", file=sys.stderr)
                    code = INFRA_EXIT if error.transient else 1
                    break
                usage.write(json.dumps(row) + "\n")
                transcript.write(json.dumps({"t": time.time(), "type": "assistant", "text": text, "calls": [{"name": n, "input": a} for _, n, a in calls]}) + "\n")
                final = text
                if not calls:
                    break
                outputs = [(i, run_tool(n, a, child_env)) for i, n, a in calls]
                for (_, n, a), (_, out) in zip(calls, outputs):
                    transcript.write(json.dumps({"t": time.time(), "type": "tool_result", "name": n, "output": out}) + "\n")
                chat.results(outputs)
    finally:
        if MCP:
            MCP.close()
    sys.stdout.write(final)
    return code


if __name__ == "__main__":
    sys.exit(main())
