from __future__ import annotations

import os
import urllib.parse
import urllib.request

from greenfloor.config.models import ProgramConfig
from greenfloor.core.notifications import AlertEvent

PUSHOVER_URL = "https://api.pushover.net/1/messages.json"


def render_low_inventory_message(event: AlertEvent) -> str:
    return (
        f"[{event.market_id}] Running low on {event.ticker}. "
        f"Remaining: {event.remaining_amount}. "
        f"Send more to receive address: {event.receive_address}."
    )


def send_pushover_alert(program: ProgramConfig, event: AlertEvent) -> None:
    if not program.pushover_enabled:
        return

    user_key = os.getenv(program.pushover_user_key_env) or os.getenv(
        program.pushover_recipient_key_env
    )
    app_token = os.getenv(program.pushover_app_token_env)
    if not user_key or not app_token:
        return

    payload = urllib.parse.urlencode(
        {
            "token": app_token,
            "user": user_key,
            "title": f"GreenFloor Low Inventory: {event.ticker}",
            "message": render_low_inventory_message(event),
            "priority": "0",
        }
    ).encode("utf-8")

    req = urllib.request.Request(PUSHOVER_URL, data=payload, method="POST")
    with urllib.request.urlopen(req, timeout=10):
        return
