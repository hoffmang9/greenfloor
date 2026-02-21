#!/bin/bash

# Simple script to run market maker with virtual environment
# Usage: ./run_with_venv.sh [additional arguments]

# Change to script directory
cd /Users/hoffmang/beta/chia-carbon-market-maker || exit 1

# Activate virtual environment
source /Users/hoffmang/beta/chia-blockchain/venv/bin/activate

# Run the market maker with any passed arguments
python3 splash_carbon_market_maker_xch.py "$@"

