#!/usr/bin/env python3
"""
Create expiring offers for carbon credits against XCH.

This script creates market-making offers for carbon credit CATs,
automatically calculating prices based on USD rates and current XCH prices.
"""

import asyncio
import sys
from typing import Optional

import click

from common import (
    calculate_expiry_time,
    cancel_offer,
    create_chia_offer,
    format_currency,
    get_carbon_price,
    get_xch_price,
    post_offer_to_splash,
    validate_wallet_id,
    xch_to_mojos,
)


DEFAULT_ACCEPT_WALLET = "1"  # XCH wallet ID
DEFAULT_EXPIRY_MINUTES = 5


@click.command("Create an expiring offer")
@click.option(
    "--offer-wallet",
    required=True,
    help="Wallet ID of the CAT being offered",
    type=str,
)
@click.option(
    "--accept-wallet",
    default=DEFAULT_ACCEPT_WALLET,
    help="Wallet ID of the XCH wallet",
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
    default=DEFAULT_EXPIRY_MINUTES,
    help="Expire in this many minutes",
    type=int,
)
@click.option(
    "--repeat",
    default=1,
    help="Make multiple of the same offer",
    type=int,
)
def main(
    offer_wallet: str,
    accept_wallet: str,
    count: int,
    expiry_minutes: int,
    repeat: int,
) -> None:
    """Create expiring offers for carbon credits."""
    try:
        # Validate inputs
        if not validate_wallet_id(offer_wallet):
            raise ValueError(f"Invalid offer wallet ID: {offer_wallet}")
        
        if count <= 0:
            raise ValueError("Count must be positive")
        
        if expiry_minutes <= 0:
            raise ValueError("Expiry minutes must be positive")
        
        if repeat <= 0:
            raise ValueError("Repeat count must be positive")
        
        # Create offers
        for i in range(repeat):
            click.echo(f"Creating offer {i + 1} of {repeat}")
            asyncio.run(
                calculate_and_create_offer(
                    offer_wallet, accept_wallet, count, expiry_minutes
                )
            )
            
    except ValueError as e:
        click.echo(f"Validation error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


async def calculate_and_create_offer(
    offer_wallet: str, accept_wallet: str, count: int, expiry_minutes: int
) -> None:
    """Calculate prices and create a single offer."""
    try:
        # Get prices
        usd_price = get_carbon_price(offer_wallet)
        xch_price = await get_xch_price()
        
        # Calculate offer details
        xch_price_per_ton = round(usd_price / xch_price, 2)
        mojo_price_per_ton = xch_to_mojos(xch_price_per_ton)
        total_ask = round(mojo_price_per_ton * count)
        expiry_time = calculate_expiry_time(expiry_minutes)
        
        # Display pricing info
        click.echo(f"USD Price per ton: {format_currency(usd_price)}")
        click.echo(f"XCH price: {format_currency(xch_price)}")
        click.echo(f"Price per ton in XCH: {xch_price_per_ton}")
        
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
        
        # Cancel the offer (as per original logic)
        cancel_offer(trade_id)
        
        # Post to Splash
        response = await post_offer_to_splash(offer_text)
        click.echo(f"Splash response: {response}")
        
    except Exception as e:
        click.echo(f"Failed to create offer: {e}", err=True)
        raise


if __name__ == "__main__":
    main()
