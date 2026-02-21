#!/usr/bin/env python3
"""
Create expiring offers for carbon credits.

This script creates market-making offers for carbon credit CATs against
either XCH (with dynamic pricing) or bridged USDC (with fixed pricing).
"""

import asyncio
import sys
from enum import Enum
from typing import Optional

import click

from common import (
    calculate_expiry_time,
    calculate_expiry_time_hours,
    cancel_offer,
    create_chia_offer,
    format_currency,
    get_carbon_price,
    get_xch_price,
    post_offer_to_splash,
    validate_wallet_id,
    xch_to_mojos,
)


class PairType(str, Enum):
    """Supported trading pairs."""
    XCH = "xch"
    USDC = "usdc"


# Default wallet IDs
DEFAULT_XCH_WALLET = "1"
DEFAULT_USDC_WALLET = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"

# Default expiry times
DEFAULT_EXPIRY_MINUTES = 65
DEFAULT_EXPIRY_HOURS = 24


@click.command("Create expiring offers for carbon credits")
@click.option(
    "--offer-wallet",
    required=True,
    help="Asset ID of the CAT being offered",
    type=str,
)
@click.option(
    "--pair",
    type=click.Choice([PairType.XCH, PairType.USDC], case_sensitive=False),
    default=PairType.XCH,
    help="Trading pair: 'xch' for XCH or 'usdc' for bridged USDC",
)
@click.option(
    "--accept-wallet",
    help="Wallet ID of the asset being requested (auto-detected if not specified)",
    type=str,
)
@click.option(
    "--count",
    default=1,
    help="How many CATs to offer",
    type=int,
)
@click.option(
    "--expiry-minutes",
    help="Expire in this many minutes (used for XCH pairs, overrides --expiry-hours)",
    type=int,
)
@click.option(
    "--expiry-hours",
    help="Expire in this many hours (used for USDC pairs if --expiry-minutes not set)",
    type=int,
)
@click.option(
    "--repeat",
    default=1,
    help="Make multiple of the same offer",
    type=int,
)
@click.option(
    "--cancel-after-create",
    is_flag=True,
    default=False,
    help="Cancel the offer after creating (useful for market making)",
)
def main(
    offer_wallet: str,
    pair: PairType,
    accept_wallet: Optional[str],
    count: int,
    expiry_minutes: Optional[int],
    expiry_hours: Optional[int],
    repeat: int,
    cancel_after_create: bool,
) -> None:
    """Create expiring offers for carbon credits against XCH or USDC."""
    try:
        # Validate inputs
        if not validate_wallet_id(offer_wallet):
            raise ValueError(f"Invalid offer wallet ID: {offer_wallet}")
        
        if count <= 0:
            raise ValueError("Count must be positive")
        
        if repeat <= 0:
            raise ValueError("Repeat count must be positive")
        
        # Determine accept wallet
        if accept_wallet:
            if not validate_wallet_id(accept_wallet):
                raise ValueError(f"Invalid accept wallet ID: {accept_wallet}")
        else:
            accept_wallet = get_default_accept_wallet(pair)
        
        # Determine expiry time
        if expiry_minutes is not None:
            if expiry_minutes <= 0:
                raise ValueError("Expiry minutes must be positive")
            expiry_config = ("minutes", expiry_minutes)
        elif expiry_hours is not None:
            if expiry_hours <= 0:
                raise ValueError("Expiry hours must be positive")
            expiry_config = ("hours", expiry_hours)
        else:
            # Use defaults based on pair type
            if pair == PairType.XCH:
                expiry_config = ("minutes", DEFAULT_EXPIRY_MINUTES)
            else:
                expiry_config = ("hours", DEFAULT_EXPIRY_HOURS)
        
        # Create offers
        click.echo(f"Creating {repeat} offer(s) for {pair.upper()} pair")
        click.echo(f"Offer wallet: {offer_wallet}")
        click.echo(f"Accept wallet: {accept_wallet}")
        click.echo(f"Expiry: {expiry_config[1]} {expiry_config[0]}")
        
        for i in range(repeat):
            if repeat > 1:
                click.echo(f"\nCreating offer {i + 1} of {repeat}")
            
            asyncio.run(
                create_offer_for_pair(
                    offer_wallet=offer_wallet,
                    accept_wallet=accept_wallet,
                    pair=pair,
                    count=count,
                    expiry_config=expiry_config,
                    cancel_after_create=cancel_after_create,
                )
            )
            
    except ValueError as e:
        click.echo(f"Validation error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


def get_default_accept_wallet(pair: PairType) -> str:
    """Get the default accept wallet for a given pair type."""
    if pair == PairType.XCH:
        return DEFAULT_XCH_WALLET
    elif pair == PairType.USDC:
        return DEFAULT_USDC_WALLET
    else:
        raise ValueError(f"Unknown pair type: {pair}")


async def create_offer_for_pair(
    offer_wallet: str,
    accept_wallet: str,
    pair: PairType,
    count: int,
    expiry_config: tuple[str, int],
    cancel_after_create: bool,
) -> None:
    """Create a single offer for the specified pair type."""
    try:
        if pair == PairType.XCH:
            await create_xch_offer(
                offer_wallet, accept_wallet, count, expiry_config, cancel_after_create
            )
        elif pair == PairType.USDC:
            await create_usdc_offer(
                offer_wallet, accept_wallet, count, expiry_config, cancel_after_create
            )
        else:
            raise ValueError(f"Unsupported pair type: {pair}")
            
    except Exception as e:
        click.echo(f"Failed to create offer: {e}", err=True)
        raise


async def create_xch_offer(
    offer_wallet: str,
    accept_wallet: str,
    count: int,
    expiry_config: tuple[str, int],
    cancel_after_create: bool,
) -> None:
    """Create an offer against XCH with dynamic pricing."""
    # Get prices
    usd_price = get_carbon_price(offer_wallet)
    xch_price = await get_xch_price()
    
    # Calculate offer details
    xch_price_per_ton = round(usd_price / xch_price, 2)
    mojo_price_per_ton = xch_to_mojos(xch_price_per_ton)
    total_ask = round(mojo_price_per_ton * count)
    
    # Calculate expiry time
    time_unit, time_value = expiry_config
    if time_unit == "minutes":
        expiry_time = calculate_expiry_time(time_value)
    else:  # hours
        expiry_time = calculate_expiry_time_hours(time_value)
    
    # Display pricing info
    click.echo(f"USD price per ton: {format_currency(usd_price)}")
    click.echo(f"XCH price: {format_currency(xch_price)}")
    click.echo(f"Price per ton in XCH: {xch_price_per_ton}")
    click.echo(f"Total asking: {xch_price_per_ton * count} XCH")
    
    # Create the offer
    offer_text, trade_id = create_chia_offer(
        offer_wallet=offer_wallet,
        accept_wallet=accept_wallet,
        count=count,
        total_ask=total_ask,
        expiry_time=expiry_time,
    )
    
    click.echo(f"Offer created: {offer_text}")
    click.echo(f"Trade ID: {trade_id}")
    
    # Cancel if requested (for market making strategy)
    if cancel_after_create:
        cancel_offer(trade_id)
    
    # Post to Splash
    response = await post_offer_to_splash(offer_text)
    click.echo(f"Splash response: {response}")


async def create_usdc_offer(
    offer_wallet: str,
    accept_wallet: str,
    count: int,
    expiry_config: tuple[str, int],
    cancel_after_create: bool,
) -> None:
    """Create an offer against bridged USDC with fixed pricing."""
    # Get USD price (no XCH conversion needed for USDC)
    usd_price = get_carbon_price(offer_wallet)
    
    # Calculate offer details (USDC is in mojos, so multiply by 1000)
    total_ask = round(usd_price * count * 1000)
    
    # Calculate expiry time
    time_unit, time_value = expiry_config
    if time_unit == "minutes":
        expiry_time = calculate_expiry_time(time_value)
    else:  # hours
        expiry_time = calculate_expiry_time_hours(time_value)
    
    # Display pricing info
    click.echo(f"Price per ton: {format_currency(usd_price)}")
    click.echo(f"Total asking: {format_currency(usd_price * count)}")
    
    # Create the offer
    offer_text, trade_id = create_chia_offer(
        offer_wallet=offer_wallet,
        accept_wallet=accept_wallet,
        count=count,
        total_ask=total_ask,
        expiry_time=expiry_time,
    )
    
    click.echo(f"Offer created: {offer_text}")
    click.echo(f"Trade ID: {trade_id}")
    
    # Cancel if requested (though less common for USDC offers)
    if cancel_after_create:
        cancel_offer(trade_id)
    
    # Post to Splash
    response = await post_offer_to_splash(offer_text)
    click.echo(f"Splash response: {response}")


if __name__ == "__main__":
    main()

