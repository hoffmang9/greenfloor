from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True, slots=True)
class SignerKeyConfig:
    key_id: str
    fingerprint: int
    network: str | None = None
    keyring_yaml_path: str | None = None


@dataclass(slots=True)
class ProgramConfig:
    app_network: str
    home_dir: str
    runtime_loop_interval_seconds: int
    runtime_dry_run: bool
    tx_block_webhook_enabled: bool
    tx_block_webhook_listen_addr: str
    dexie_api_base: str
    splash_api_base: str
    offer_publish_venue: str
    coin_ops_max_operations_per_run: int
    coin_ops_max_daily_fee_budget_mojos: int
    coin_ops_split_fee_mojos: int
    coin_ops_combine_fee_mojos: int
    python_min_version: str
    low_inventory_enabled: bool
    low_inventory_threshold_mode: str
    low_inventory_default_threshold_base_units: int
    low_inventory_dedup_cooldown_seconds: int
    low_inventory_clear_hysteresis_percent: int
    pushover_enabled: bool
    pushover_user_key_env: str
    pushover_app_token_env: str
    pushover_recipient_key_env: str
    signer_key_registry: dict[str, SignerKeyConfig] = field(default_factory=dict)


@dataclass(slots=True)
class MarketInventoryConfig:
    low_watermark_base_units: int
    low_inventory_alert_threshold_base_units: int | None = None
    current_available_base_units: int = 0
    bucket_counts: dict[int, int] = field(default_factory=dict)


@dataclass(slots=True)
class MarketLadderEntry:
    size_base_units: int
    target_count: int
    split_buffer_count: int
    combine_when_excess_factor: float


@dataclass(slots=True)
class MarketConfig:
    market_id: str
    enabled: bool
    base_asset: str
    base_symbol: str
    quote_asset: str
    quote_asset_type: str
    receive_address: str
    mode: str
    signer_key_id: str
    inventory: MarketInventoryConfig
    pricing: dict[str, Any] = field(default_factory=dict)
    ladders: dict[str, list[MarketLadderEntry]] = field(default_factory=dict)


@dataclass(slots=True)
class MarketsConfig:
    markets: list[MarketConfig] = field(default_factory=list)


def _req(mapping: dict[str, Any], key: str) -> Any:
    if key not in mapping:
        raise ValueError(f"Missing required field: {key}")
    return mapping[key]


def _validate_strategy_pricing(pricing: dict[str, Any], market_id: str) -> None:
    spread_raw = pricing.get("strategy_target_spread_bps")
    if spread_raw is not None:
        try:
            spread = int(spread_raw)
        except (TypeError, ValueError) as exc:
            raise ValueError(
                f"market {market_id}: strategy_target_spread_bps must be an integer"
            ) from exc
        if spread <= 0:
            raise ValueError(f"market {market_id}: strategy_target_spread_bps must be positive")

    min_raw = pricing.get("strategy_min_xch_price_usd")
    max_raw = pricing.get("strategy_max_xch_price_usd")
    min_price: float | None = None
    max_price: float | None = None
    if min_raw is not None:
        try:
            min_price = float(min_raw)
        except (TypeError, ValueError) as exc:
            raise ValueError(
                f"market {market_id}: strategy_min_xch_price_usd must be numeric"
            ) from exc
        if min_price <= 0:
            raise ValueError(f"market {market_id}: strategy_min_xch_price_usd must be > 0")
    if max_raw is not None:
        try:
            max_price = float(max_raw)
        except (TypeError, ValueError) as exc:
            raise ValueError(
                f"market {market_id}: strategy_max_xch_price_usd must be numeric"
            ) from exc
        if max_price <= 0:
            raise ValueError(f"market {market_id}: strategy_max_xch_price_usd must be > 0")
    if min_price is not None and max_price is not None and min_price > max_price:
        raise ValueError(
            f"market {market_id}: strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd"
        )


def parse_program_config(raw: dict[str, Any]) -> ProgramConfig:
    app = _req(raw, "app")
    runtime = _req(raw, "runtime")
    chain_signals = _req(raw, "chain_signals")
    tx_trigger = _req(chain_signals, "tx_block_trigger")
    venues = raw.get("venues", {})
    dexie = venues.get("dexie", {})
    splash = venues.get("splash", {})
    offer_publish = venues.get("offer_publish", {})
    coin_ops = raw.get("coin_ops", {})
    dev = _req(raw, "dev")
    notifications = _req(raw, "notifications")
    low = _req(notifications, "low_inventory_alerts")
    providers = _req(notifications, "providers")
    pushover = next((p for p in providers if p.get("type") == "pushover"), None)
    if pushover is None:
        raise ValueError("Missing notifications.providers entry with type=pushover")
    key_registry: dict[str, SignerKeyConfig] = {}
    keys_root = raw.get("keys", {})
    registry_rows = keys_root.get("registry", [])
    if registry_rows is None:
        registry_rows = []
    if not isinstance(registry_rows, list):
        raise ValueError("keys.registry must be a list")
    for row in registry_rows:
        if not isinstance(row, dict):
            raise ValueError("keys.registry entries must be mappings")
        key_id = str(_req(row, "key_id")).strip()
        if not key_id:
            raise ValueError("keys.registry entry key_id must be non-empty")
        try:
            fingerprint = int(_req(row, "fingerprint"))
        except (TypeError, ValueError) as exc:
            raise ValueError(f"invalid fingerprint for key_id={key_id}") from exc
        if fingerprint <= 0:
            raise ValueError(f"fingerprint for key_id={key_id} must be positive")
        if key_id in key_registry:
            raise ValueError(f"duplicate key_id in keys.registry: {key_id}")
        network = str(row.get("network", "")).strip() or None
        keyring_yaml_path = str(row.get("keyring_yaml_path", "")).strip() or None
        key_registry[key_id] = SignerKeyConfig(
            key_id=key_id,
            fingerprint=fingerprint,
            network=network,
            keyring_yaml_path=keyring_yaml_path,
        )

    offer_publish_venue = str(offer_publish.get("provider", "dexie")).strip().lower()
    if offer_publish_venue not in {"dexie", "splash"}:
        raise ValueError("venues.offer_publish.provider must be one of: dexie, splash")

    return ProgramConfig(
        app_network=str(_req(app, "network")),
        home_dir=str(_req(app, "home_dir")),
        runtime_loop_interval_seconds=int(_req(runtime, "loop_interval_seconds")),
        runtime_dry_run=bool(runtime.get("dry_run", False)),
        tx_block_webhook_enabled=bool(_req(tx_trigger, "webhook_enabled")),
        tx_block_webhook_listen_addr=str(_req(tx_trigger, "webhook_listen_addr")),
        dexie_api_base=str(dexie.get("api_base", "https://api.dexie.space")),
        splash_api_base=str(splash.get("api_base", "http://john-deere.hoffmang.com:4000")),
        offer_publish_venue=offer_publish_venue,
        coin_ops_max_operations_per_run=int(coin_ops.get("max_operations_per_run", 20)),
        coin_ops_max_daily_fee_budget_mojos=int(coin_ops.get("max_daily_fee_budget_mojos", 0)),
        coin_ops_split_fee_mojos=int(coin_ops.get("split_fee_mojos", 0)),
        coin_ops_combine_fee_mojos=int(coin_ops.get("combine_fee_mojos", 0)),
        python_min_version=str(_req(dev["python"], "min_version")),
        low_inventory_enabled=bool(_req(low, "enabled")),
        low_inventory_threshold_mode=str(_req(low, "threshold_mode")),
        low_inventory_default_threshold_base_units=int(_req(low, "default_threshold_base_units")),
        low_inventory_dedup_cooldown_seconds=int(_req(low, "dedup_cooldown_seconds")),
        low_inventory_clear_hysteresis_percent=int(_req(low, "clear_hysteresis_percent")),
        pushover_enabled=bool(_req(pushover, "enabled")),
        pushover_user_key_env=str(_req(pushover, "user_key_env")),
        pushover_app_token_env=str(_req(pushover, "app_token_env")),
        pushover_recipient_key_env=str(_req(pushover, "recipient_key_env")),
        signer_key_registry=key_registry,
    )


def parse_markets_config(raw: dict[str, Any]) -> MarketsConfig:
    market_rows = _req(raw, "markets")
    markets: list[MarketConfig] = []
    for row in market_rows:
        inventory = row.get("inventory", {})
        inv = MarketInventoryConfig(
            low_watermark_base_units=int(_req(inventory, "low_watermark_base_units")),
            low_inventory_alert_threshold_base_units=(
                int(inventory["low_inventory_alert_threshold_base_units"])
                if inventory.get("low_inventory_alert_threshold_base_units") is not None
                else None
            ),
            current_available_base_units=int(inventory.get("current_available_base_units", 0)),
            bucket_counts={
                int(k): int(v) for k, v in dict(inventory.get("bucket_counts", {})).items()
            },
        )
        raw_ladders = row.get("ladders", {})
        ladders: dict[str, list[MarketLadderEntry]] = {}
        for side, entries in dict(raw_ladders).items():
            side_entries: list[MarketLadderEntry] = []
            for e in entries:
                side_entries.append(
                    MarketLadderEntry(
                        size_base_units=int(_req(e, "size_base_units")),
                        target_count=int(_req(e, "target_count")),
                        split_buffer_count=int(e.get("split_buffer_count", 0)),
                        combine_when_excess_factor=float(e.get("combine_when_excess_factor", 2.0)),
                    )
                )
            ladders[str(side)] = side_entries
        market_id = str(_req(row, "id"))
        pricing = dict(row.get("pricing", {}))
        _validate_strategy_pricing(pricing, market_id)
        markets.append(
            MarketConfig(
                market_id=market_id,
                enabled=bool(_req(row, "enabled")),
                base_asset=str(_req(row, "base_asset")),
                base_symbol=str(_req(row, "base_symbol")),
                quote_asset=str(_req(row, "quote_asset")),
                quote_asset_type=str(_req(row, "quote_asset_type")),
                receive_address=str(_req(row, "receive_address")),
                mode=str(_req(row, "mode")),
                signer_key_id=str(_req(row, "signer_key_id")),
                inventory=inv,
                pricing=pricing,
                ladders=ladders,
            )
        )
    return MarketsConfig(markets=markets)
