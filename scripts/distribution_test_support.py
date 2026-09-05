"""Shared local release fixture helpers for channel-specific distribution tests."""

from __future__ import annotations

import hashlib
import http.server
import importlib.util
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import zipfile
from pathlib import Path
from typing import NoReturn


ROOT = Path(__file__).resolve().parent.parent
SMOKE = ROOT / "scripts" / "smoke-native.py"


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, _format: str, *_args: object) -> None:
        pass

    def copyfile(self, source, outputfile) -> None:
        try:
            super().copyfile(source, outputfile)
        except BrokenPipeError:
            pass


def fail(channel: str, message: str) -> NoReturn:
    raise SystemExit(f"{channel} distribution validation failed: {message}")


def run(channel: str, label: str, command: list[str], env: dict[str, str] | None = None) -> str:
    print(f"==> {channel}: {label}")
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            env=env,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as error:
        fail(channel, f"{label}: missing executable {error.filename}")
    except subprocess.CalledProcessError as error:
        output = "\n".join(part for part in (error.stdout, error.stderr) if part)
        if output:
            print(output, file=sys.stderr, end="" if output.endswith("\n") else "\n")
        fail(channel, f"{label}: command exited with status {error.returncode}")
    return result.stdout


def native_smoke(channel: str, wright: Path, lsp: Path, version: str) -> None:
    try:
        subprocess.run(
            [
                sys.executable,
                str(SMOKE),
                "--wright",
                str(wright),
                "--wright-lsp",
                str(lsp),
                "--version",
                version,
            ],
            cwd=ROOT,
            check=True,
        )
    except FileNotFoundError as error:
        fail(channel, f"native post-install smoke: missing executable {error.filename}")
    except subprocess.CalledProcessError as error:
        fail(channel, f"native post-install smoke: command exited with status {error.returncode}")


def load_generator():
    spec = importlib.util.spec_from_file_location(
        "wright_dist", ROOT / "scripts" / "update-dist-manifests.py"
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load scripts/update-dist-manifests.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReleaseFixture:
    def __init__(
        self,
        channel: str,
        version: str,
        target: str,
        extension: str,
        executable_suffix: str,
    ) -> None:
        self.channel = channel
        self.version = version
        self.target = target
        self.extension = extension
        self.executable_suffix = executable_suffix

    def __enter__(self) -> "ReleaseFixture":
        self._temporary = tempfile.TemporaryDirectory(prefix="wright-distribution-")
        self.work = Path(self._temporary.name)
        self.archive = self._stage_artifact()
        self.server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0),
            lambda *args, **kwargs: QuietHandler(*args, directory=str(self.work), **kwargs),
        )
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base = f"http://127.0.0.1:{self.server.server_port}/releases/download"
        self.metadata = self._generate_metadata()
        latest = self.work / "repos" / "wrightkit" / "wright" / "releases" / "latest"
        latest.parent.mkdir(parents=True, exist_ok=True)
        latest.write_text(
            f'{{"tag_name":"v{self.version}","draft":false,"prerelease":false}}\n'
        )
        return self

    def __exit__(self, exc_type, exc_value, traceback) -> None:
        self.server.shutdown()
        self.thread.join(timeout=5)
        self._temporary.cleanup()

    def _stage_artifact(self) -> Path:
        source_dir = ROOT / "target" / "debug"
        payload_name = f"wright-{self.version}-{self.target}"
        release_dir = self.work / "releases" / "download" / f"v{self.version}"
        payload = release_dir / payload_name
        payload.mkdir(parents=True)
        for name in (f"wright{self.executable_suffix}", f"wright-lsp{self.executable_suffix}"):
            source = source_dir / name
            if not source.is_file():
                fail(
                    self.channel,
                    f"native debug binary is missing: {source}; build wright-cli and wright-lsp first",
                )
            shutil.copy2(source, payload / name)
        (payload / "version.json").write_text(f'{{"version":"{self.version}"}}\n')

        archive = release_dir / f"{payload_name}.{self.extension}"
        if self.extension == "zip":
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
                for path in payload.iterdir():
                    output.write(path, f"{payload_name}/{path.name}")
        else:
            with tarfile.open(archive, "w:gz") as output:
                output.add(payload, arcname=payload_name)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        archive.with_name(f"{archive.name}.sha256").write_text(f"{digest}  {archive.name}\n")
        return archive

    def _generate_metadata(self) -> Path:
        generator = load_generator()
        hashes = {key: "" for key in generator.TARGETS}
        target_key = next(key for key, value in generator.TARGETS.items() if value == self.target)
        hashes[target_key] = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        metadata = self.work / "metadata"
        generator.generate(self.version, hashes, metadata, self.base)
        return metadata
