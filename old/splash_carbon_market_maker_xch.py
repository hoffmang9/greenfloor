#!/usr/bin/env python3
"""
Dexie Market Maker Script for XCH and USDC pairs.

This script checks CAT token pair offers on Dexie and automatically creates
market-making offers when liquidity is below target thresholds using the
unified make_offer.py script.
"""

import asyncio
import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from typing import Tuple

import aiohttp
import click

from common import validate_wallet_id


# Constants
DEFAULT_CAT_ID = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
DEFAULT_PAIR = "xch"
DEXIE_API_BASE = "https://api.dexie.space/v1/offers"

# Target thresholds for different offer sizes
ONES_TARGET = 5
TENS_TARGET = 2
HUNDREDS_TARGET = 1

# Expiry configuration by pair type
PAIR_EXPIRY_CONFIG = {
    "xch": {"unit": "minutes", "value": 65},
    "usdc": {"unit": "hours", "value": 24},
}

REQUEST_TIMEOUT = 30
SUBPROCESS_TIMEOUT = 300  # 5 minutes


@click.command("Get CAT Pairs Market")
@click.option(
    "--cat",
    default=DEFAULT_CAT_ID,
    help="Asset ID of the CAT being checked",
    type=str,
)
@click.option(
    "--pair",
    default=DEFAULT_PAIR,
    type=click.Choice(["xch", "usdc"], case_sensitive=False),
    help="Pair currency: 'xch' for XCH or 'usdc' for bridged USDC",
)
@click.option(
    "--make-offer-path",
    default="./make_offer.py",
    help="Path to the make_offer.py script",
    type=click.Path(exists=True),
)
@click.option(
    "--ones-target",
    default=ONES_TARGET,
    help="Target number of 1-unit offers",
    type=int,
)
@click.option(
    "--tens-target",
    default=TENS_TARGET,
    help="Target number of 10-unit offers",
    type=int,
)
@click.option(
    "--hundreds-target",
    default=HUNDREDS_TARGET,
    help="Target number of 100-unit offers",
    type=int,
)
@click.option(
    "--dry-run",
    is_flag=True,
    default=False,
    help="Show what would be done without creating offers",
)
def main(
    cat: str,
    pair: str,
    make_offer_path: str,
    ones_target: int,
    tens_target: int,
    hundreds_target: int,
    dry_run: bool,
) -> None:
    """Main entry point for the market making script."""
    try:
        # Validate inputs
        if not validate_wallet_id(cat):
            raise ValueError(f"Invalid CAT wallet ID: {cat}")
        
        if not Path(make_offer_path).is_file():
            raise ValueError(f"Make offer script not found: {make_offer_path}")
        
        for target in [ones_target, tens_target, hundreds_target]:
            if target < 0:
                raise ValueError("Target values must be non-negative")
        
        # Get current market state
        counts = asyncio.run(get_pair_offers(cat, pair))
        targets = (tens_target, ones_target, hundreds_target)
        
        print_status(cat, pair, counts, targets)
        
        if dry_run:
            show_dry_run_plan(cat, pair, counts, targets, make_offer_path)
        else:
            create_missing_offers(cat, pair, counts, targets, make_offer_path)
        
    except ValueError as e:
        click.echo(f"Validation error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


def print_status(
    cat: str, pair: str, counts: Tuple[int, int, int], targets: Tuple[int, int, int]
) -> None:
    """Print current market status."""
    tens, ones, hundreds = counts
    tens_target, ones_target, hundreds_target = targets
    
    click.echo("=" * 60)
    click.echo(f"Market Making Status for {pair.upper()} Pair")
    click.echo("=" * 60)
    click.echo(f"Asset ID: {cat}")
    click.echo(f"Current time: {datetime.now().isoformat()}")
    click.echo(f"Pair: {pair.upper()}")
    click.echo()
    click.echo("Market Depth Analysis:")
    click.echo(f"  1-unit offers:   {ones:2d} (target: {ones_target:2d}) {'✓' if ones >= ones_target else '⚠'}")
    click.echo(f"  10-unit offers:  {tens:2d} (target: {tens_target:2d}) {'✓' if tens >= tens_target else '⚠'}")
    click.echo(f"  100-unit offers: {hundreds:2d} (target: {hundreds_target:2d}) {'✓' if hundreds >= hundreds_target else '⚠'}")
    click.echo()


def show_dry_run_plan(
    cat: str,
    pair: str,
    counts: Tuple[int, int, int],
    targets: Tuple[int, int, int],
    make_offer_path: str,
) -> None:
    """Show what would be done in a dry run."""
    tens, ones, hundreds = counts
    tens_target, ones_target, hundreds_target = targets
    
    click.echo("DRY RUN - No offers will be created")
    click.echo("-" * 40)
    
    offer_configs = [
        {
            "current": ones,
            "target": ones_target,
            "size": 1,
            "name": "1-unit offers",
        },
        {
            "current": tens,
            "target": tens_target,
            "size": 10,
            "name": "10-unit offers",
        },
        {
            "current": hundreds,
            "target": hundreds_target,
            "size": 100,
            "name": "100-unit offers",
        },
    ]
    
    total_needed = 0
    for config in offer_configs:
        if config["current"] < config["target"]:
            needed = config["target"] - config["current"]
            total_needed += needed
            click.echo(f"Would create {needed} {config['name']}")
            click.echo(f"  Command: python {make_offer_path} --offer-wallet {cat} --pair {pair} --count {config['size']} --repeat {needed} --cancel-after-create")
    
    if total_needed == 0:
        click.echo("No offers needed - all targets met!")
    else:
        click.echo(f"\nTotal offers to create: {total_needed}")


def create_missing_offers(
    cat: str,
    pair: str,
    counts: Tuple[int, int, int],
    targets: Tuple[int, int, int],
    make_offer_path: str,
) -> None:
    """Create offers for any categories below target thresholds."""
    tens, ones, hundreds = counts
    tens_target, ones_target, hundreds_target = targets
    
    # Define offer configurations
    offer_configs = [
        {
            "current": ones,
            "target": ones_target,
            "size": 1,
            "name": "1-unit offers",
        },
        {
            "current": tens,
            "target": tens_target,
            "size": 10,
            "name": "10-unit offers",
        },
        {
            "current": hundreds,
            "target": hundreds_target,
            "size": 100,
            "name": "100-unit offers",
        },
    ]
    
    created_any = False
    for config in offer_configs:
        if config["current"] < config["target"]:
            needed = config["target"] - config["current"]
            created_any = True
            create_offers(
                make_offer_path=make_offer_path,
                cat=cat,
                pair=pair,
                count=config["size"],
                repeat=needed,
                description=config["name"],
            )
    
    if not created_any:
        click.echo("✓ All liquidity targets met - no offers needed!")


def create_offers(
    make_offer_path: str, cat: str, pair: str, count: int, repeat: int, description: str
) -> None:
    """Create market making offers using the unified make_offer script."""
    click.echo(f"Creating {repeat} {description}")
    
    # Get expiry configuration for the pair
    expiry_config = PAIR_EXPIRY_CONFIG.get(pair.lower(), PAIR_EXPIRY_CONFIG["xch"])
    expiry_arg = f"--expiry-{expiry_config['unit']}"
    expiry_value = str(expiry_config["value"])
    
    try:
        # Build command for unified make_offer script
        command = [
            "python3",  # Use python3 explicitly for better compatibility
            make_offer_path,
            "--offer-wallet", cat,
            "--pair", pair,
            "--count", str(count),
            "--repeat", str(repeat),
            "--cancel-after-create",  # Always cancel for market making
            expiry_arg, expiry_value,
        ]
        
        click.echo(f"Running: {' '.join(command)}")
        
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=SUBPROCESS_TIMEOUT,
            check=False,
        )
        
        if result.returncode != 0:
            if result.stderr:
                click.echo(f"❌ Error creating offers: {result.stderr.strip()}", err=True)
            else:
                click.echo(
                    f"❌ Offer creation failed with return code: {result.returncode}",
                    err=True,
                )
        else:
            click.echo("✓ Offers created successfully")
            if result.stdout:
                # Show relevant output, but filter out verbose details
                lines = result.stdout.strip().split('\n')
                for line in lines:
                    if any(keyword in line.lower() for keyword in 
                          ['created', 'response', 'price', 'total', 'error', 'failed']):
                        click.echo(f"  {line}")
            
    except subprocess.TimeoutExpired:
        click.echo("❌ Error: Offer creation timed out", err=True)
    except Exception as e:
        click.echo(f"❌ Error running make_offer script: {e}", err=True)


async def get_pair_offers(cat: str, pair: str) -> Tuple[int, int, int]:
    """
    Fetch offer data from Dexie API and count offers by size.
    
    Args:
        cat: The CAT asset ID being offered
        pair: The pair currency ("xch" or "usdc")
    
    Returns:
        Tuple of (tens_count, ones_count, hundreds_count)
    """
    # Map pair names to Dexie API format
    pair_mapping = {
        "xch": "xch",
        "usdc": "wUSDC.b",  # Bridged USDC identifier on Dexie
    }
    
    dexie_pair = pair_mapping.get(pair.lower(), pair)
    url = f"{DEXIE_API_BASE}?offered={cat}&requested={dexie_pair}"
    
    ones = tens = hundreds = 0
    
    try:
        timeout = aiohttp.ClientTimeout(total=REQUEST_TIMEOUT)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            click.echo(f"Fetching market data from: {url}")
            async with session.get(url) as response:
                response.raise_for_status()
                pair_data = await response.json()
                
                offers = pair_data.get("offers", [])
                if not offers:
                    click.echo("ℹ️  No offers found in the market")
                    return (tens, ones, hundreds)
                
                click.echo(f"Found {len(offers)} offers to analyze")
                
                for offer in offers:
                    offered_items = offer.get("offered", [])
                    if not offered_items:
                        continue
                        
                    amount = offered_items[0].get("amount")
                    if amount == 1:
                        ones += 1
                    elif amount == 10:
                        tens += 1
                    elif amount == 100:
                        hundreds += 1
                
                return (tens, ones, hundreds)
                
    except aiohttp.ClientError as e:
        raise Exception(f"Failed to fetch data from Dexie API: {e}") from e
    except json.JSONDecodeError as e:
        raise Exception(f"Failed to parse API response: {e}") from e
    except Exception as e:
        raise Exception(f"Unexpected error fetching pair data: {e}") from e


if __name__ == "__main__":
    main()

