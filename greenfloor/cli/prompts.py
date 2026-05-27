"""Interactive CLI prompt helpers."""

from __future__ import annotations

import sys


def should_prompt_for_override(prompt_for_override: bool | None) -> bool:
    if prompt_for_override is not None:
        return bool(prompt_for_override)
    return bool(sys.stdin.isatty() and sys.stdout.isatty())


def prompt_yes_no(message: str, *, prompt_for_override: bool | None) -> bool:
    if not should_prompt_for_override(prompt_for_override):
        return False
    try:
        answer = input(f"{message} [y/N]: ").strip().lower()
    except EOFError:
        return False
    return answer in {"y", "yes"}
