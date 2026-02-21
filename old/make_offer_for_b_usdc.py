#!/usr/bin/env python3
"""
Create expiring offers for carbon credits against bridged USDC.

This script creates market-making offers for carbon credit CATs against
bridged USDC, using fixed USD pricing without XCH conversion.
"""

import asyncio
import sys

import click

from common import (
    calculate_expiry_time_hours,
    create_chia_offer,
    format_currency,
    get_carbon_price,
    post_offer_to_splash,
    validate_wallet_id,
)


# Default bridged USDC CAT ID
DEFAULT_ACCEPT_WALLET = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"
DEFAULT_EXPIRY_HOURS = 24


@click.command("Create an expiring offer")
@click.option(
    "--offer-wallet",
    required=True,
    help="Asset ID of the CAT being offered",
    type=str,
)
@click.option(
    "--accept-wallet",
    default=DEFAULT_ACCEPT_WALLET,
    help="Wallet ID of the CAT being requested (bridged USDC)",
    type=str,
)
@click.option(
    "--count",
    default=1,
    help="How many CATs to offer",
    type=int,
)
@click.option(
    "--expiry-hours",
    default=DEFAULT_EXPIRY_HOURS,
    help="Expire in this many hours",
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
    expiry_hours: int,
    repeat: int,
) -> None:
    """Create expiring offers for carbon credits against bridged USDC."""
    try:
        # Validate inputs
        if not validate_wallet_id(offer_wallet):
            raise ValueError(f"Invalid offer wallet ID: {offer_wallet}")
        
        if not validate_wallet_id(accept_wallet):
            raise ValueError(f"Invalid accept wallet ID: {accept_wallet}")
        
        if count <= 0:
            raise ValueError("Count must be positive")
        
        if expiry_hours <= 0:
            raise ValueError("Expiry hours must be positive")
        
        if repeat <= 0:
            raise ValueError("Repeat count must be positive")
        
        # Create offers
        for i in range(repeat):
            click.echo(f"Creating offer {i + 1} of {repeat}")
            asyncio.run(
                calculate_and_create_offer(
                    offer_wallet, accept_wallet, count, expiry_hours
                )
            )
            
    except ValueError as e:
        click.echo(f"Validation error: {e}", err=True)
        sys.exit(1)
    except Exception as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)


async def calculate_and_create_offer(
    offer_wallet: str, accept_wallet: str, count: int, expiry_hours: int
) -> None:
    """Calculate prices and create a single offer for bridged USDC."""
    try:
        # Get USD price (no XCH conversion needed for USDC)
        usd_price = get_carbon_price(offer_wallet)
        
        # Calculate offer details (USDC is in mojos, so multiply by 1000)
        total_ask = round(usd_price * count * 1000)
        expiry_time = calculate_expiry_time_hours(expiry_hours)
        
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
        
        # Post to Splash
        response = await post_offer_to_splash(offer_text)
        click.echo(f"Splash response: {response}")
        
    except Exception as e:
        click.echo(f"Failed to create offer: {e}", err=True)
        raise


if __name__ == "__main__":
    main()

