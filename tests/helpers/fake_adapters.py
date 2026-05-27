from __future__ import annotations


class FakeDexie:
    """Minimal Dexie stub for orchestration tests (post + visibility verify)."""

    offer_id = "offer-123"

    def __init__(self, base_url: str) -> None:
        self.base_url = base_url

    def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None = None):
        self.last_offer = offer
        self.drop_only = drop_only
        self.claim_rewards = claim_rewards
        return {"success": True, "id": self.offer_id}

    def get_offer(self, offer_id: str) -> dict:
        return {
            "offer": {
                "id": offer_id,
                "offered": [{"id": "a1", "code": "a1", "name": "A1"}],
                "requested": [{"id": "xch", "code": "xch", "name": "xch"}],
            }
        }
