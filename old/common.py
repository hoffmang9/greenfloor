#!/usr/bin/env python3
"""
Common functions for Chia carbon credit market making scripts.

This module contains shared functionality for creating offers, posting to APIs,
and interacting with the Chia blockchain.
"""

import json
import os
import subprocess
import time
from pathlib import Path
from typing import Dict, Optional, Union

import aiohttp


# Constants
CHIA_BIN_PATH = "/Users/hoffmang/beta/chia-blockchain/venv/bin/chia"
SPLASH_API_URL = os.getenv(
    "SPLASH_API_URL", "http://john-deere.hoffmang.com:4000"
).rstrip("/")
XCH_PRICE_API_URL = "https://coincodex.com/api/coincodex/get_coin/xch"
DEFAULT_FEE = 0
REQUEST_TIMEOUT = 30
RPC_TIMEOUT = 30

# Carbon credit pricing configuration
CARBON_CREDIT_PRICES = {
    # Agricultural Reforestation Project 2022
    "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7": 7.75,
    # Agricultural Reforestation Project 2020
    "e257aca547a83020e537e87f8c83e9332d2c3adb729c052e6f04971317084327": 7.50,
    # Antioquia and Caldas Reforestation 2022
    "9720fcb8333984c72f914fc5090509ae9f7b1ff72eff2ed6825d944d7a571066": 6.75,
    # Smallholder Reforestation Project 2022
    "9d9264c542c2a3108c7b8f74cad82b60dcbb6e50b328e9cdbaa7acb468a5707f": 6.75,
    # Smallholder Reforestation Project 2021
    "5af8db0b15e0de99ad1eff02486bb1998602053c56dfb22dc04e0f5e17ccec8d": 6.50,
}
DEFAULT_CARBON_PRICE = 7.75


def get_chia_bin_path() -> str:
    """Get the path to the chia binary."""
    return CHIA_BIN_PATH


def get_carbon_price(wallet_id: str) -> float:
    """Get the USD price per carbon credit for a given wallet ID."""
    return CARBON_CREDIT_PRICES.get(wallet_id, DEFAULT_CARBON_PRICE)


async def get_xch_price() -> float:
    """Fetch the current XCH price in USD from CoinCodex API."""
    timeout = aiohttp.ClientTimeout(total=REQUEST_TIMEOUT)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        try:
            async with session.get(XCH_PRICE_API_URL) as response:
                response.raise_for_status()
                xch_data = await response.json()
                return float(xch_data["last_price_usd"])
        except (aiohttp.ClientError, KeyError, ValueError) as e:
            raise Exception(f"Failed to fetch XCH price: {e}") from e


def run_chia_rpc(
    endpoint: str, data: Optional[Union[str, Dict]] = None
) -> Optional[Dict]:
    """Run a chia RPC command and return the parsed JSON result."""
    chia_bin = get_chia_bin_path()
    
    if data is None:
        data_str = "{}"
    elif isinstance(data, dict):
        data_str = json.dumps(data)
    else:
        data_str = str(data)
    
    try:
        result = subprocess.run(
            [chia_bin, "rpc", "wallet", endpoint, data_str],
            capture_output=True,
            text=True,
            timeout=RPC_TIMEOUT,
            check=False
        )
        
        if result.returncode != 0:
            print(f"Error running RPC command: {result.stderr.strip()}")
            return None
            
        return json.loads(result.stdout)
        
    except subprocess.TimeoutExpired:
        print(f"RPC command timed out for endpoint: {endpoint}")
        return None
    except json.JSONDecodeError as e:
        print(f"Failed to parse JSON response: {e}")
        print(f"Raw output: {result.stdout}")
        return None
    except Exception as e:
        print(f"Unexpected error running RPC: {e}")
        return None


def cancel_offer(trade_id: str) -> bool:
    """Cancel a specific offer by trade_id with secure=false."""
    print(f"Canceling offer: {trade_id}")
    
    cancel_data = {
        "trade_id": trade_id,
        "secure": False,
        "fee": DEFAULT_FEE
    }
    
    response = run_chia_rpc("cancel_offer", cancel_data)
    
    if response is None:
        print(f"  Failed to cancel offer {trade_id}")
        return False
    
    if response.get("success", False):
        print(f"  Successfully canceled offer {trade_id}")
        return True
    
    print(f"  Failed to cancel offer {trade_id}: {response}")
    return False


def create_offer_rpc_command(
    offer_wallet: str,
    accept_wallet: str,
    count: int,
    total_ask: int,
    expiry_time: int,
    fee: int = DEFAULT_FEE
) -> str:
    """Create the RPC command JSON for creating an offer."""
    offer_data = {
        "offer": {
            str(accept_wallet): total_ask,
            offer_wallet: -abs(count * 1000)  # Negative and in mojos
        },
        "fee": fee,
        "driver_dict": {},
        "validate_only": False,
        "reuse_puzhash": True,
        "max_time": expiry_time
    }
    return json.dumps(offer_data)


def create_chia_offer(
    offer_wallet: str,
    accept_wallet: str,
    count: int,
    total_ask: int,
    expiry_time: int
) -> tuple[str, str]:
    """
    Create a Chia offer and return the offer text and trade ID.
    
    Returns:
        Tuple of (offer_text, trade_id)
    """
    chia_bin = get_chia_bin_path()
    rpc_command = create_offer_rpc_command(
        offer_wallet, accept_wallet, count, total_ask, expiry_time
    )
    
    print(f"Creating offer with command: {rpc_command}")
    
    try:
        result = subprocess.run(
            [chia_bin, "rpc", "wallet", "create_offer_for_ids", rpc_command],
            capture_output=True,
            text=True,
            timeout=RPC_TIMEOUT,
            check=False
        )
        
        if result.returncode != 0:
            err_msg = f"{result.stderr.strip()}. Code: {result.returncode}"
            raise Exception(err_msg)
        
        json_object = json.loads(result.stdout)
        offer_text = json_object["offer"]
        trade_id = json_object["trade_record"]["trade_id"]
        
        return offer_text, trade_id
        
    except subprocess.TimeoutExpired as e:
        raise Exception("Offer creation timed out") from e
    except json.JSONDecodeError as e:
        raise Exception(f"Failed to parse offer response: {e}") from e
    except KeyError as e:
        raise Exception(f"Unexpected offer response format: {e}") from e


async def post_offer_to_splash(offer_text: str) -> Dict:
    """Post an offer to the Splash API."""
    json_offer = json.dumps({"offer": offer_text})
    
    timeout = aiohttp.ClientTimeout(total=REQUEST_TIMEOUT)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        try:
            async with session.post(
                SPLASH_API_URL,
                data=json_offer,
                headers={"Content-Type": "application/json"}
            ) as response:
                response.raise_for_status()
                return await response.json()
        except aiohttp.ClientError as e:
            raise Exception(f"Failed to post offer to Splash: {e}") from e


def calculate_expiry_time(minutes: int) -> int:
    """Calculate the expiry time in Unix timestamp."""
    return int(time.time()) + (minutes * 60)


def calculate_expiry_time_hours(hours: int) -> int:
    """Calculate the expiry time in Unix timestamp from hours."""
    return int(time.time()) + (hours * 3600)


def mojos_to_xch(mojos: int) -> float:
    """Convert mojos to XCH."""
    return mojos / 1_000_000_000_000


def xch_to_mojos(xch: float) -> int:
    """Convert XCH to mojos."""
    return int(xch * 1_000_000_000_000)


def validate_wallet_id(wallet_id: str) -> bool:
    """Validate that a wallet ID is a valid hex string of correct length."""
    if not wallet_id:
        return False
    
    try:
        # Chia asset IDs should be 64 characters (32 bytes) of hex
        if len(wallet_id) != 64:
            return False
        int(wallet_id, 16)  # Validate it's valid hex
        return True
    except ValueError:
        return False


def format_currency(amount: float, currency: str = "USD") -> str:
    """Format currency amount for display."""
    return f"{amount:.2f} {currency}"
