from __future__ import annotations

import json
import urllib.request
from typing import Any


class SplashAdapter:
    def __init__(self, base_url: str) -> None:
        self.base_url = base_url.rstrip("/")

    def post_offer(self, offer: str) -> dict[str, Any]:
        payload = {"offer": offer}
        body = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            self.base_url,
            data=body,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=30) as resp:
            result = json.loads(resp.read().decode("utf-8"))
        if isinstance(result, dict):
            return result
        return {"success": False, "error": "invalid_response_format"}
