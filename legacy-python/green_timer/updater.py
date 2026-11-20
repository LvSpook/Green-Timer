"""Update checking and installer download.

Only HTTPS GitHub API/download URLs are accepted, redirects are followed
manually so the final scheme can be verified, downloads are size-capped and
the result is written atomically.
"""

from __future__ import annotations

import json
import logging
import os
import re
import tempfile
import urllib.error
import urllib.request
from typing import Optional, Tuple

from . import GITHUB_REPO

log = logging.getLogger(__name__)

API_URL = f"https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
USER_AGENT = "GreenTimer-Updater"
MAX_DOWNLOAD_BYTES = 200 * 1024 * 1024  # 200 MiB
MAX_REDIRECTS = 5


def parse_version(tag: str) -> Tuple[int, ...]:
    """Turn ``v1.2.3`` / ``1.2`` into a comparable tuple, ignoring suffixes."""
    numbers = re.findall(r"\d+", tag or "")
    if not numbers:
        return ()
    return tuple(int(part) for part in numbers[:4])


def is_newer(candidate: str, current: str) -> bool:
    return parse_version(candidate) > parse_version(current)


def _require_https(url: str) -> None:
    if not url.lower().startswith("https://"):
        raise ValueError(f"refusing non-HTTPS URL: {url}")


def _open(url: str, timeout: float = 10.0):
    _require_https(url)
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    # Follow redirects manually so each hop can be validated.
    opener = urllib.request.build_opener()
    hop = 0
    response = opener.open(request, timeout=timeout)
    while getattr(response, "status", 200) in (301, 302, 303, 307, 308) and hop < MAX_REDIRECTS:
        location = response.headers.get("Location")
        response.close()
        if not location:
            raise ValueError("redirect without Location header")
        _require_https(location)
        request = urllib.request.Request(location, headers={"User-Agent": USER_AGENT})
        response = opener.open(request, timeout=timeout)
        hop += 1
    return response


def check_for_update(current_version: str, timeout: float = 8.0) -> Optional[Tuple[str, str]]:
    """Return ``(latest_tag, download_url)`` when a newer release exists."""
    if not GITHUB_REPO or "/" not in GITHUB_REPO:
        return None
    try:
        with _open(API_URL, timeout=timeout) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, ValueError, OSError) as exc:
        log.info("Update check skipped: %s", exc)
        return None

    tag = payload.get("tag_name", "")
    if not tag or not is_newer(tag, current_version):
        return None

    for asset in payload.get("assets", []):
        name = asset.get("name", "")
        url = asset.get("browser_download_url", "")
        if name.lower().endswith(".exe") and url:
            return tag, url
    return None


def download_installer(url: str, timeout: float = 30.0) -> str:
    """Download an installer to a temp file and return its path."""
    handle, path = tempfile.mkstemp(prefix="GreenTimer-Setup-", suffix=".exe")
    os.close(handle)
    try:
        written = 0
        with _open(url, timeout=timeout) as response, open(path, "wb") as out:
            while True:
                chunk = response.read(64 * 1024)
                if not chunk:
                    break
                written += len(chunk)
                if written > MAX_DOWNLOAD_BYTES:
                    raise ValueError("installer exceeds size limit")
                out.write(chunk)
        if written == 0:
            raise ValueError("download was empty")
        return path
    except BaseException:
        try:
            os.unlink(path)
        except OSError:
            pass
        raise
