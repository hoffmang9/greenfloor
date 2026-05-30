from __future__ import annotations

import warnings
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal

from greenfloor.logging_setup import normalize_log_level_name

OfferExecutionBackend = Literal["signer"]
ManagedOfferExecutionBackend = Literal["signer"]
CoinOpsExecutionBackend = Literal["signer"]

_CANONICAL_CAT_UNIT_MOJOS = 1000
_CANONICAL_XCH_UNIT_MOJOS = 1_000_000_000_000
_XCH_UNIT_SYMBOLS = frozenset({"xch", "txch", "1"})


@dataclass(frozen=True, slots=True)
class SignerKeyConfig:
    key_id: str
    fingerprint: int
    network: str | None = None
    keyring_yaml_path: str | None = None


@dataclass(frozen=True, slots=True)
class VaultWalletKeyConfig:
    public_key_hex: str
    curve: str


@dataclass(frozen=True, slots=True)
class VaultConfig:
    launcher_id: str
    custody_threshold: int
    recovery_threshold: int
    recovery_clawback_timelock: int
    custody_keys: tuple[VaultWalletKeyConfig, ...]
    recovery_keys: tuple[VaultWalletKeyConfig, ...]


_SIGNER_CONFIG_FILENAME = "signer.yaml"
_DEFAULT_SIGNER_COINSET_MSP_BASE_URL = "https://api-msp.coinset.org"


def signer_config_path(home_dir: str) -> Path:
    return Path(home_dir).expanduser() / "config" / _SIGNER_CONFIG_FILENAME


def build_signer_config_document(program: ProgramConfig) -> dict[str, Any]:
    if program.vault_config is None:
        raise ValueError("vault config is required to build signer config document")
    if not str(program.signer_kms_key_id).strip():
        raise ValueError("signer.kms_key_id is required to build signer config document")
    signer_section: dict[str, Any] = {
        "kms_key_id": str(program.signer_kms_key_id).strip(),
    }
    kms_region = str(program.signer_kms_region or "").strip()
    if kms_region:
        signer_section["kms_region"] = kms_region
    kms_public_key_hex = str(program.signer_kms_public_key_hex or "").strip()
    if kms_public_key_hex:
        signer_section["kms_public_key_hex"] = kms_public_key_hex
    coinset_msp_base_url = str(program.signer_coinset_msp_base_url or "").strip()
    if coinset_msp_base_url:
        signer_section["coinset_msp_base_url"] = coinset_msp_base_url
    vault = program.vault_config
    return {
        "app": {"network": str(program.app_network).strip()},
        "signer": signer_section,
        "vault": {
            "launcher_id": vault.launcher_id,
            "custody_threshold": int(vault.custody_threshold),
            "recovery_threshold": int(vault.recovery_threshold),
            "recovery_clawback_timelock": int(vault.recovery_clawback_timelock),
            "custody_keys": [
                {"public_key_hex": key.public_key_hex, "curve": key.curve}
                for key in vault.custody_keys
            ],
            "recovery_keys": [
                {"public_key_hex": key.public_key_hex, "curve": key.curve}
                for key in vault.recovery_keys
            ],
        },
    }


def write_signer_config_file(program: ProgramConfig) -> Path:
    from greenfloor.config.io import write_yaml

    path = signer_config_path(program.home_dir)
    write_yaml(path, build_signer_config_document(program))
    return path


_prepared_signer_config_by_home: dict[str, str] = {}


def invalidate_signer_runtime_cache(*, home_dir: str | None = None) -> None:
    """Drop cached signer.yaml path(s) after program config reload."""
    if home_dir is None:
        _prepared_signer_config_by_home.clear()
        return
    home_key = str(Path(home_dir).expanduser().resolve())
    _prepared_signer_config_by_home.pop(home_key, None)


def prepare_signer_runtime(program: ProgramConfig) -> str:
    """Write signer.yaml once per home_dir for the process lifetime."""
    home_key = str(Path(program.home_dir).expanduser().resolve())
    cached = _prepared_signer_config_by_home.get(home_key)
    if cached:
        return cached
    path = str(write_signer_config_file(program))
    _prepared_signer_config_by_home[home_key] = path
    return path


def signer_runtime_config_path(program: ProgramConfig) -> str:
    """Return signer.yaml path without writing."""
    home_key = str(Path(program.home_dir).expanduser().resolve())
    cached = _prepared_signer_config_by_home.get(home_key)
    if cached:
        return cached
    return str(signer_config_path(program.home_dir))


def signer_offer_path_configured(program: ProgramConfig) -> bool:
    if not str(program.signer_kms_key_id).strip():
        return False
    vault = program.vault_config
    return vault is not None and bool(str(vault.launcher_id).strip())


def require_signer_offer_path(program: ProgramConfig) -> None:
    if not signer_offer_path_configured(program):
        raise ValueError(
            "offer execution requires signer.kms_key_id and vault.launcher_id in program config"
        )


def coin_ops_execution_backend(program: ProgramConfig) -> CoinOpsExecutionBackend:
    require_signer_offer_path(program)
    return "signer"


def offer_execution_backend(
    program: ProgramConfig,
    *,
    size_base_units: int = 0,
    local_build_min_size_base_units: int | None = None,
) -> OfferExecutionBackend:
    del size_base_units, local_build_min_size_base_units
    require_signer_offer_path(program)
    return "signer"


def managed_offer_execution_backend(
    program: ProgramConfig,
    *,
    size_base_units: int = 0,
    local_build_min_size_base_units: int | None = None,
) -> ManagedOfferExecutionBackend:
    del size_base_units, local_build_min_size_base_units
    require_signer_offer_path(program)
    return "signer"


@dataclass(slots=True)
class ProgramConfig:
    app_network: str
    home_dir: str
    runtime_loop_interval_seconds: int
    runtime_dry_run: bool
    tx_block_trigger_mode: str
    tx_block_websocket_url: str
    tx_block_websocket_reconnect_interval_seconds: int
    tx_block_fallback_poll_interval_seconds: int
    tx_block_webhook_enabled: bool
    tx_block_webhook_listen_addr: str
    dexie_api_base: str
    splash_api_base: str
    offer_publish_venue: str
    coin_ops_max_operations_per_run: int
    coin_ops_max_daily_fee_budget_mojos: int
    coin_ops_minimum_fee_mojos: int
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
    runtime_parallel_markets: bool = False
    runtime_market_slot_count: int = 0
    runtime_offer_parallelism_enabled: bool = False
    runtime_offer_parallelism_max_workers: int = 4
    runtime_reservation_ttl_seconds: int = 300
    runtime_offer_bootstrap_wait_timeout_seconds: int = 120
    app_log_level: str = "INFO"
    app_log_level_was_missing: bool = False
    signer_key_registry: dict[str, SignerKeyConfig] = field(default_factory=dict)
    signer_coinset_msp_base_url: str = _DEFAULT_SIGNER_COINSET_MSP_BASE_URL
    signer_kms_key_id: str = ""
    signer_kms_region: str = ""
    signer_kms_public_key_hex: str = ""
    vault_config: VaultConfig | None = None


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
    cancel_move_threshold_bps: int | None = None
    ladders: dict[str, list[MarketLadderEntry]] = field(default_factory=dict)


@dataclass(slots=True)
class MarketsConfig:
    markets: list[MarketConfig] = field(default_factory=list)


def _req(mapping: dict[str, Any], key: str) -> Any:
    if key not in mapping:
        raise ValueError(f"Missing required field: {key}")
    return mapping[key]


def _runtime_timeout_seconds(
    runtime: dict[str, Any],
    *,
    neutral_key: str,
    legacy_key: str,
    default: int,
    minimum: int,
) -> int:
    for key in (neutral_key, legacy_key):
        if key in runtime:
            return max(minimum, int(runtime[key]))
    return max(minimum, default)


def _parse_vault_wallet_keys(rows: Any, *, section: str) -> tuple[VaultWalletKeyConfig, ...]:
    if rows is None:
        raise ValueError(f"Missing required field: vault.{section}")
    if not isinstance(rows, list):
        raise ValueError(f"vault.{section} must be a list")
    parsed: list[VaultWalletKeyConfig] = []
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError(f"vault.{section} entries must be mappings")
        public_key_hex = str(_req(row, "public_key_hex")).strip()
        curve = str(_req(row, "curve")).strip()
        if not public_key_hex:
            raise ValueError(f"vault.{section} public_key_hex must be non-empty")
        if not curve:
            raise ValueError(f"vault.{section} curve must be non-empty")
        parsed.append(VaultWalletKeyConfig(public_key_hex=public_key_hex, curve=curve))
    if not parsed:
        raise ValueError(f"vault.{section} must contain at least one key")
    return tuple(parsed)


def _parse_vault_config(raw: dict[str, Any] | None) -> VaultConfig | None:
    if raw is None:
        return None
    if not isinstance(raw, dict):
        raise ValueError("vault must be a mapping")
    launcher_id = str(_req(raw, "launcher_id")).strip()
    if not launcher_id:
        raise ValueError("vault.launcher_id must be non-empty")
    return VaultConfig(
        launcher_id=launcher_id,
        custody_threshold=int(_req(raw, "custody_threshold")),
        recovery_threshold=int(_req(raw, "recovery_threshold")),
        recovery_clawback_timelock=int(_req(raw, "recovery_clawback_timelock")),
        custody_keys=_parse_vault_wallet_keys(raw.get("custody_keys"), section="custody_keys"),
        recovery_keys=_parse_vault_wallet_keys(raw.get("recovery_keys"), section="recovery_keys"),
    )


def _parse_signer_section(raw: dict[str, Any] | None) -> dict[str, str]:
    if raw is None:
        return {
            "coinset_msp_base_url": _DEFAULT_SIGNER_COINSET_MSP_BASE_URL,
            "kms_key_id": "",
            "kms_region": "",
            "kms_public_key_hex": "",
        }
    if not isinstance(raw, dict):
        raise ValueError("signer must be a mapping")
    return {
        "coinset_msp_base_url": str(
            raw.get("coinset_msp_base_url", _DEFAULT_SIGNER_COINSET_MSP_BASE_URL)
        ).strip()
        or _DEFAULT_SIGNER_COINSET_MSP_BASE_URL,
        "kms_key_id": str(raw.get("kms_key_id", "")).strip(),
        "kms_region": str(raw.get("kms_region", "")).strip(),
        "kms_public_key_hex": str(raw.get("kms_public_key_hex", "")).strip(),
    }


def _validate_strategy_pricing(
    pricing: dict[str, Any], market_id: str, quote_asset_type: str | None = None
) -> None:
    quote_type = str(quote_asset_type or "").strip().lower()
    for legacy_field in ("reference_source", "reference_pair"):
        if pricing.get(legacy_field) is not None:
            raise ValueError(f"market {market_id}: {legacy_field} is no longer supported")

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

    if (
        pricing.get("strategy_offer_expiry_unit") is not None
        or pricing.get("strategy_offer_expiry_value") is not None
    ):
        raise ValueError(
            f"market {market_id}: strategy_offer_expiry_unit/value are no longer supported; use strategy_offer_expiry_minutes"
        )

    expiry_minutes_raw = pricing.get("strategy_offer_expiry_minutes")
    if expiry_minutes_raw is not None:
        try:
            expiry_minutes = int(expiry_minutes_raw)
        except (TypeError, ValueError) as exc:
            raise ValueError(
                f"market {market_id}: strategy_offer_expiry_minutes must be an integer"
            ) from exc
        if expiry_minutes <= 0:
            raise ValueError(f"market {market_id}: strategy_offer_expiry_minutes must be positive")
        if quote_type == "unstable" and expiry_minutes > 15:
            warnings.warn(
                f"market {market_id}: unstable strategy_offer_expiry_minutes={expiry_minutes} exceeds 15 minutes",
                stacklevel=2,
            )

    threshold_raw = pricing.get("cancel_move_threshold_bps")
    if threshold_raw is not None:
        try:
            threshold = int(threshold_raw)
        except (TypeError, ValueError) as exc:
            raise ValueError(
                f"market {market_id}: cancel_move_threshold_bps must be an integer"
            ) from exc
        if threshold <= 0:
            raise ValueError(f"market {market_id}: cancel_move_threshold_bps must be positive")


def _uses_cat_units(asset_id: str) -> bool:
    normalized = str(asset_id).strip().lower()
    return bool(normalized) and normalized not in _XCH_UNIT_SYMBOLS


def canonicalize_asset_unit_mojo_multiplier(
    *,
    asset_id: str,
    raw_value: Any,
    field_name: str,
    market_id: str,
) -> int:
    if raw_value in (None, ""):
        if str(asset_id).strip().lower() in _XCH_UNIT_SYMBOLS:
            return _CANONICAL_XCH_UNIT_MOJOS
        return _CANONICAL_CAT_UNIT_MOJOS
    try:
        multiplier = int(raw_value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"market {market_id}: {field_name} must be an integer") from exc
    if multiplier <= 0:
        raise ValueError(f"market {market_id}: {field_name} must be positive")
    if _uses_cat_units(asset_id) and multiplier != _CANONICAL_CAT_UNIT_MOJOS:
        raise ValueError(f"market {market_id}: {field_name} must be 1000 for CAT assets")
    return multiplier


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

    if raw.get("cloud_wallet") not in (None, {}):
        raise ValueError(
            "cloud_wallet config is removed; use signer: and vault: blocks instead "
            "(see config/program.yaml)"
        )

    signer_fields = _parse_signer_section(raw.get("signer"))
    vault_config = _parse_vault_config(raw.get("vault"))

    coin_ops_minimum_fee_mojos = int(coin_ops.get("minimum_fee_mojos", 10_000_000))
    if coin_ops_minimum_fee_mojos < 0:
        raise ValueError("coin_ops.minimum_fee_mojos must be >= 0")
    tx_block_trigger_mode = str(tx_trigger.get("mode", "websocket")).strip().lower()
    if tx_block_trigger_mode != "websocket":
        raise ValueError("chain_signals.tx_block_trigger.mode must be websocket")
    tx_block_websocket_url = str(tx_trigger.get("websocket_url", "")).strip()
    if not tx_block_websocket_url:
        app_network = str(_req(app, "network")).strip().lower()
        if app_network in {"testnet", "testnet11"}:
            tx_block_websocket_url = "wss://testnet11.api.coinset.org/ws"
        else:
            tx_block_websocket_url = "wss://api.coinset.org/ws"
    tx_block_websocket_reconnect_interval_seconds = int(
        tx_trigger.get("websocket_reconnect_interval_seconds", 30)
    )
    if tx_block_websocket_reconnect_interval_seconds < 1:
        raise ValueError(
            "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds must be >= 1"
        )
    tx_block_fallback_poll_interval_seconds = int(
        tx_trigger.get("fallback_poll_interval_seconds", 60)
    )
    if tx_block_fallback_poll_interval_seconds < 0:
        raise ValueError(
            "chain_signals.tx_block_trigger.fallback_poll_interval_seconds must be >= 0"
        )
    app_log_level_was_missing = "log_level" not in app
    app_log_level = normalize_log_level_name(app.get("log_level"))

    return ProgramConfig(
        app_network=str(_req(app, "network")),
        home_dir=str(_req(app, "home_dir")),
        runtime_loop_interval_seconds=int(_req(runtime, "loop_interval_seconds")),
        runtime_dry_run=bool(runtime.get("dry_run", False)),
        runtime_parallel_markets=bool(runtime.get("parallel_markets", False)),
        runtime_market_slot_count=max(0, int(runtime.get("market_slot_count", 0))),
        runtime_offer_parallelism_enabled=bool(runtime.get("offer_parallelism_enabled", False)),
        runtime_offer_parallelism_max_workers=max(
            1, int(runtime.get("offer_parallelism_max_workers", 4))
        ),
        runtime_reservation_ttl_seconds=max(30, int(runtime.get("reservation_ttl_seconds", 300))),
        tx_block_trigger_mode=tx_block_trigger_mode,
        tx_block_websocket_url=tx_block_websocket_url,
        tx_block_websocket_reconnect_interval_seconds=tx_block_websocket_reconnect_interval_seconds,
        tx_block_fallback_poll_interval_seconds=tx_block_fallback_poll_interval_seconds,
        tx_block_webhook_enabled=bool(tx_trigger.get("webhook_enabled", False)),
        tx_block_webhook_listen_addr=str(tx_trigger.get("webhook_listen_addr", "127.0.0.1:8787")),
        dexie_api_base=str(dexie.get("api_base", "https://api.dexie.space")),
        splash_api_base=str(splash.get("api_base", "http://john-deere.hoffmang.com:4000")),
        offer_publish_venue=offer_publish_venue,
        coin_ops_max_operations_per_run=int(coin_ops.get("max_operations_per_run", 20)),
        coin_ops_max_daily_fee_budget_mojos=int(coin_ops.get("max_daily_fee_budget_mojos", 0)),
        coin_ops_minimum_fee_mojos=coin_ops_minimum_fee_mojos,
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
        runtime_offer_bootstrap_wait_timeout_seconds=_runtime_timeout_seconds(
            runtime,
            neutral_key="offer_bootstrap_wait_timeout_seconds",
            legacy_key="cloud_wallet_bootstrap_wait_timeout_seconds",
            default=120,
            minimum=10,
        ),
        app_log_level=app_log_level,
        app_log_level_was_missing=app_log_level_was_missing,
        signer_key_registry=key_registry,
        signer_coinset_msp_base_url=signer_fields["coinset_msp_base_url"],
        signer_kms_key_id=signer_fields["kms_key_id"],
        signer_kms_region=signer_fields["kms_region"],
        signer_kms_public_key_hex=signer_fields["kms_public_key_hex"],
        vault_config=vault_config,
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
        pricing["base_unit_mojo_multiplier"] = canonicalize_asset_unit_mojo_multiplier(
            asset_id=str(_req(row, "base_asset")),
            raw_value=pricing.get("base_unit_mojo_multiplier"),
            field_name="base_unit_mojo_multiplier",
            market_id=market_id,
        )
        pricing["quote_unit_mojo_multiplier"] = canonicalize_asset_unit_mojo_multiplier(
            asset_id=str(_req(row, "quote_asset")),
            raw_value=pricing.get("quote_unit_mojo_multiplier"),
            field_name="quote_unit_mojo_multiplier",
            market_id=market_id,
        )
        _validate_strategy_pricing(
            pricing,
            market_id,
            quote_asset_type=str(row.get("quote_asset_type", "")).strip().lower(),
        )
        threshold_raw = pricing.pop("cancel_move_threshold_bps", None)
        cancel_move_threshold_bps = int(threshold_raw) if threshold_raw is not None else None
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
                cancel_move_threshold_bps=cancel_move_threshold_bps,
                ladders=ladders,
            )
        )
    return MarketsConfig(markets=markets)
