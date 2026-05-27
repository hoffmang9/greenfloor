"""GreenFloor manager CLI entry point."""

from __future__ import annotations

import argparse
from pathlib import Path

from greenfloor.cli.cats import cats_add, cats_delete, cats_list
from greenfloor.cli.coin_ops import (
    coin_combine,
    coin_split,
    coin_status,
    coins_list,
    seed_wallet_assets_cache_cli,
)
from greenfloor.cli.keys_onboard import keys_onboard
from greenfloor.cli.manager_setup import (
    bootstrap_home,
    doctor,
    set_log_level,
    validate_config,
)
from greenfloor.cli.offer_build_post import (
    build_and_post_offer_cli,
    resolve_offer_publish_settings,
)
from greenfloor.cli.offers_lifecycle import offers_cancel, offers_reconcile, offers_status
from greenfloor.config.io import (
    default_cats_config_path as _default_cats_config_path_shared,
)
from greenfloor.config.io import (
    default_state_dir_path as _default_state_dir_path_shared,
)
from greenfloor.runtime.json_output import set_json_output_compact


def _default_program_config_path() -> str:
    home_default = Path("~/.greenfloor/config/program.yaml").expanduser()
    if home_default.exists():
        return str(home_default)
    return "config/program.yaml"


def _default_markets_config_path() -> str:
    home_default = Path("~/.greenfloor/config/markets.yaml").expanduser()
    if home_default.exists():
        return str(home_default)
    return "config/markets.yaml"


def _default_testnet_markets_config_path() -> str:
    home_default = Path("~/.greenfloor/config/testnet-markets.yaml").expanduser()
    if home_default.exists():
        return str(home_default)
    return ""


def _default_cats_config_path() -> str:
    shared = _default_cats_config_path_shared()
    return str(shared) if shared is not None else "config/cats.yaml"


def main() -> None:
    parser = argparse.ArgumentParser(description="GreenFloor manager CLI")
    parser.add_argument("--program-config", default=_default_program_config_path())
    parser.add_argument("--markets-config", default=_default_markets_config_path())
    parser.add_argument("--testnet-markets-config", default=_default_testnet_markets_config_path())
    parser.add_argument("--cats-config", default=_default_cats_config_path())
    parser.add_argument("--state-db", default="")
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit compact single-line JSON output (default: pretty JSON).",
    )

    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("config-validate")

    p_onboard = sub.add_parser("keys-onboard")
    p_onboard.add_argument("--chia-keys-dir", default="")
    p_onboard.add_argument("--key-id", required=True)
    p_onboard.add_argument("--state-dir", default=str(_default_state_dir_path_shared()))

    p_build_post = sub.add_parser("build-and-post-offer")
    group_market = p_build_post.add_mutually_exclusive_group(required=True)
    group_market.add_argument("--market-id", default="")
    group_market.add_argument(
        "--pair",
        default="",
        help="Pair selector in base:quote or base/quote form",
    )
    p_build_post.add_argument("--size-base-units", required=True, type=int)
    p_build_post.add_argument("--repeat", default=1, type=int)
    p_build_post.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_build_post.add_argument("--dexie-base-url", default="")
    p_build_post.add_argument(
        "--allow-take",
        action="store_true",
        help="Set drop_only=false when posting",
    )
    p_build_post.add_argument("--claim-rewards", action="store_true")
    p_build_post.add_argument("--dry-run", action="store_true")
    p_build_post.add_argument("--venue", choices=["dexie", "splash"], default=None)
    p_build_post.add_argument("--splash-base-url", default="")

    sub.add_parser("doctor")

    p_offers_status = sub.add_parser("offers-status")
    p_offers_status.add_argument("--market-id", default="")
    p_offers_status.add_argument("--limit", type=int, default=50)
    p_offers_status.add_argument("--events-limit", type=int, default=30)

    p_offers_reconcile = sub.add_parser("offers-reconcile")
    p_offers_reconcile.add_argument("--market-id", default="")
    p_offers_reconcile.add_argument("--limit", type=int, default=200)
    p_offers_reconcile.add_argument("--venue", choices=["dexie", "splash"], default=None)

    p_offers_cancel = sub.add_parser("offers-cancel")
    p_offers_cancel.add_argument("--offer-id", action="append", default=[])
    p_offers_cancel.add_argument("--cancel-open", action="store_true")
    p_offers_cancel.add_argument("--submit-onchain-after-offchain", action="store_true")
    p_offers_cancel.add_argument("--onchain-market-id", default="")
    p_offers_cancel.add_argument("--onchain-pair", default="")

    p_bootstrap = sub.add_parser("bootstrap-home")
    p_bootstrap.add_argument("--home-dir", default="~/.greenfloor")
    p_bootstrap.add_argument("--program-template", default="config/program.yaml")
    p_bootstrap.add_argument("--markets-template", default="config/markets.yaml")
    p_bootstrap.add_argument("--cats-template", default="config/cats.yaml")
    p_bootstrap.add_argument("--testnet-markets-template", default="config/testnet-markets.yaml")
    p_bootstrap.add_argument("--seed-testnet-markets", action="store_true")
    p_bootstrap.add_argument("--force", action="store_true")

    p_cats_add = sub.add_parser("cats-add")
    p_cats_add.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_cats_add.add_argument("--cat-id", default="")
    p_cats_add.add_argument("--ticker", default="")
    p_cats_add.add_argument("--name", default="")
    p_cats_add.add_argument("--base-symbol", default="")
    p_cats_add.add_argument("--ticker-id", default="")
    p_cats_add.add_argument("--pool-id", default="")
    p_cats_add.add_argument("--last-price-xch", default="")
    p_cats_add.add_argument("--target-usd-per-unit", default="")
    p_cats_add.add_argument("--no-dexie-lookup", action="store_true")
    p_cats_add.add_argument("--replace", action="store_true")

    sub.add_parser("cats-list")

    p_cats_delete = sub.add_parser("cats-delete")
    p_cats_delete.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_cats_delete.add_argument("--cat-id", default="")
    p_cats_delete.add_argument("--ticker", default="")
    p_cats_delete.add_argument("--no-dexie-lookup", action="store_true")
    p_cats_delete.add_argument("--yes", action="store_true")
    p_cats_delete.add_argument("--preflight-only", action="store_true")

    p_set_log_level = sub.add_parser("set-log-level")
    p_set_log_level.add_argument("--log-level", required=True)

    p_coins_list = sub.add_parser("coins-list")
    p_coins_list.add_argument("--asset", default="")
    p_coins_list.add_argument("--vault-id", default="")
    p_coins_list.add_argument("--cat-id", default="", help="hex CAT asset_id to filter by")

    p_seed_assets = sub.add_parser(
        "seed-wallet-assets-cache",
        help="Fetch resolveWalletAssets once and write ~/.greenfloor/cache/wallet_assets_*.json",
    )
    p_seed_assets.add_argument("--vault-id", default="")

    p_coin_status = sub.add_parser("coin-status")
    p_coin_status.add_argument("--asset", default="")
    p_coin_status.add_argument("--vault-id", default="")
    p_coin_status.add_argument("--cat-id", default="", help="hex CAT asset_id to filter by")

    p_coin_split = sub.add_parser("coin-split")
    split_market_group = p_coin_split.add_mutually_exclusive_group(required=True)
    split_market_group.add_argument("--market-id", default="")
    split_market_group.add_argument("--pair", default="")
    p_coin_split.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_coin_split.add_argument("--coin-id", action="append", default=[])
    p_coin_split.add_argument("--amount-per-coin", default=0, type=int)
    p_coin_split.add_argument("--number-of-coins", default=0, type=int)
    p_coin_split.add_argument("--size-base-units", default=0, type=int)
    p_coin_split.add_argument("--venue", choices=["dexie", "splash"], default=None)
    p_coin_split.add_argument("--until-ready", action="store_true")
    p_coin_split.add_argument("--max-iterations", default=3, type=int)
    p_coin_split.add_argument("--no-wait", action="store_true")
    p_coin_split.add_argument("--allow-lock-all-spendable", action="store_true")
    p_coin_split.add_argument("--force-split-when-ready", action="store_true")

    p_coin_combine = sub.add_parser("coin-combine")
    combine_market_group = p_coin_combine.add_mutually_exclusive_group(required=True)
    combine_market_group.add_argument("--market-id", default="")
    combine_market_group.add_argument("--pair", default="")
    p_coin_combine.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_coin_combine.add_argument(
        "--input-coin-count",
        dest="input_coin_count",
        default=0,
        type=int,
        help="Number of input coins to combine.",
    )
    p_coin_combine.add_argument("--asset-id", default="")
    p_coin_combine.add_argument("--coin-id", action="append", default=[])
    p_coin_combine.add_argument("--size-base-units", default=0, type=int)
    p_coin_combine.add_argument("--venue", choices=["dexie", "splash"], default=None)
    p_coin_combine.add_argument("--until-ready", action="store_true")
    p_coin_combine.add_argument("--max-iterations", default=3, type=int)
    p_coin_combine.add_argument("--no-wait", action="store_true")

    args = parser.parse_args()
    testnet_markets_path = (
        Path(args.testnet_markets_config) if str(args.testnet_markets_config).strip() else None
    )
    set_json_output_compact(bool(args.json))
    if args.command == "config-validate":
        code = validate_config(
            Path(args.program_config),
            Path(args.markets_config),
            testnet_markets_path,
        )
    elif args.command == "keys-onboard":
        code = keys_onboard(
            program_path=Path(args.program_config),
            key_id=args.key_id,
            state_dir=Path(args.state_dir).expanduser(),
            chia_keys_dir=Path(args.chia_keys_dir).expanduser()
            if str(args.chia_keys_dir).strip()
            else None,
        )
    elif args.command == "build-and-post-offer":
        venue, dexie_base_url, splash_base_url = resolve_offer_publish_settings(
            program_path=Path(args.program_config),
            network=args.network,
            venue_override=args.venue,
            dexie_base_url=args.dexie_base_url or None,
            splash_base_url=args.splash_base_url or None,
        )
        code = build_and_post_offer_cli(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            testnet_markets_path=testnet_markets_path,
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            size_base_units=args.size_base_units,
            repeat=args.repeat,
            publish_venue=venue,
            dexie_base_url=dexie_base_url,
            splash_base_url=splash_base_url,
            drop_only=not bool(args.allow_take),
            claim_rewards=bool(args.claim_rewards),
            dry_run=bool(args.dry_run),
        )
    elif args.command == "doctor":
        code = doctor(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            state_db=args.state_db or None,
            testnet_markets_path=testnet_markets_path,
        )
    elif args.command == "offers-status":
        code = offers_status(
            program_path=Path(args.program_config),
            state_db=args.state_db or None,
            market_id=args.market_id or None,
            limit=int(args.limit),
            events_limit=int(args.events_limit),
        )
    elif args.command == "offers-reconcile":
        code = offers_reconcile(
            program_path=Path(args.program_config),
            state_db=args.state_db or None,
            market_id=args.market_id or None,
            limit=int(args.limit),
            venue=args.venue,
        )
    elif args.command == "offers-cancel":
        code = offers_cancel(
            program_path=Path(args.program_config),
            offer_ids=[str(value) for value in args.offer_id],
            cancel_open=bool(args.cancel_open),
            markets_path=Path(args.markets_config),
            testnet_markets_path=testnet_markets_path,
            submit_onchain_after_offchain=bool(args.submit_onchain_after_offchain),
            onchain_market_id=args.onchain_market_id or None,
            onchain_pair=args.onchain_pair or None,
        )
    elif args.command == "bootstrap-home":
        code = bootstrap_home(
            home_dir=Path(args.home_dir),
            program_template=Path(args.program_template),
            markets_template=Path(args.markets_template),
            cats_template=Path(args.cats_template) if str(args.cats_template).strip() else None,
            testnet_markets_template=(
                Path(args.testnet_markets_template)
                if str(args.testnet_markets_template).strip()
                else None
            ),
            seed_testnet_markets=bool(args.seed_testnet_markets),
            force=bool(args.force),
        )
    elif args.command == "cats-add":
        code = cats_add(
            cats_path=Path(args.cats_config),
            network=args.network,
            cat_id=args.cat_id or None,
            ticker=args.ticker or None,
            name=args.name or None,
            base_symbol=args.base_symbol or None,
            ticker_id=args.ticker_id or None,
            pool_id=args.pool_id or None,
            last_price_xch=args.last_price_xch or None,
            target_usd_per_unit=args.target_usd_per_unit or None,
            use_dexie_lookup=not bool(args.no_dexie_lookup),
            replace=bool(args.replace),
        )
    elif args.command == "cats-list":
        code = cats_list(cats_path=Path(args.cats_config))
    elif args.command == "cats-delete":
        code = cats_delete(
            cats_path=Path(args.cats_config),
            network=args.network,
            cat_id=args.cat_id or None,
            ticker=args.ticker or None,
            use_dexie_lookup=not bool(args.no_dexie_lookup),
            confirm_delete=bool(args.yes),
            preflight_only=bool(args.preflight_only),
        )
    elif args.command == "set-log-level":
        code = set_log_level(
            program_path=Path(args.program_config),
            log_level=args.log_level,
        )
    elif args.command == "coins-list":
        code = coins_list(
            program_path=Path(args.program_config),
            asset=args.asset or None,
            vault_id=args.vault_id or None,
            cat_id=args.cat_id or None,
        )
    elif args.command == "seed-wallet-assets-cache":
        code = seed_wallet_assets_cache_cli(
            program_path=Path(args.program_config),
            vault_id=args.vault_id or None,
        )
    elif args.command == "coin-status":
        code = coin_status(
            program_path=Path(args.program_config),
            asset=args.asset or None,
            vault_id=args.vault_id or None,
            cat_id=args.cat_id or None,
        )
    elif args.command == "coin-split":
        code = coin_split(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            testnet_markets_path=testnet_markets_path,
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            coin_ids=[str(value) for value in args.coin_id],
            amount_per_coin=int(args.amount_per_coin),
            number_of_coins=int(args.number_of_coins),
            no_wait=bool(args.no_wait),
            venue=args.venue,
            size_base_units=int(args.size_base_units) or None,
            until_ready=bool(args.until_ready),
            max_iterations=int(args.max_iterations),
            allow_lock_all_spendable=bool(args.allow_lock_all_spendable),
            force_split_when_ready=bool(args.force_split_when_ready),
        )
    elif args.command == "coin-combine":
        code = coin_combine(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            testnet_markets_path=testnet_markets_path,
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            number_of_coins=int(args.input_coin_count),
            asset_id=args.asset_id or None,
            coin_ids=[str(value) for value in args.coin_id],
            no_wait=bool(args.no_wait),
            venue=args.venue,
            size_base_units=int(args.size_base_units) or None,
            until_ready=bool(args.until_ready),
            max_iterations=int(args.max_iterations),
        )
    else:
        raise ValueError(f"unsupported command: {args.command}")
    raise SystemExit(code)


if __name__ == "__main__":
    main()
